//! The probes of scene mode (`kjk sim probes scene`), each on the proving
//! ground with a navigation mesh over the half players arrive in, a
//! conversation started by a script (`probe_begin`, a party one) or by a
//! click (a player's own), and the probes' scripts and DLGs written here.

use super::super::{Avatar, Fighting, Id, Player, SimWorld, Talking};
use crate::action::{Action, Verb};
use crate::combat::Sheet;
use crate::combat::idle::Acting;
use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::module::{Body, ObjectData, ObjectKind, Placement, Repute};
use crate::probe::check;
use crate::script::actions::Target;
use crate::script::dialogue::{Pick, REPLY_ANGLE, answer};
use crate::snapshot::{SimId, Snapshot};
use crate::world::{StepReason, WorldConfig};
use collision::UNIT;
use fmt_odyssey::dlg::{Dlg, Link, Node};
use fmt_odyssey::gff::LocString;
use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::vm::ObjectId;
use std::collections::BTreeMap;
use std::sync::Arc;

/// What a probe found.
type Found = Result<String, String>;
/// A probe's name and what it found.
type Case = (&'static str, Found);
/// A probe's name and what runs it.
type Probe = (&'static str, fn() -> Found);

/// Scene mode's checks (`kjk sim probes scene`). Later parts of scene mode
/// add theirs to `PROBES`.
pub fn all() -> Vec<Case> {
    PROBES.iter().map(|(name, run)| (*name, run())).collect()
}

const PROBES: &[Probe] = &[
    ("bystander_holds_ground", bystander_holds_ground),
    ("companion_stays", companion_stays),
    ("party_cannot_use", party_cannot_use),
    ("party_queue_cannot_open", party_queue_cannot_open),
    ("party_rolls_nothing", party_rolls_nothing),
    ("cutscene_attack_lands", cutscene_attack_lands),
    ("cutscene_blow_weapon", cutscene_blow_weapon),
    ("cutscene_shot_rounds", cutscene_shot_rounds),
    ("cutscene_shot_at_object", cutscene_shot_at_object),
    ("script_push_throws", script_push_throws),
    ("assured_shot_rounds", assured_shot_rounds),
    ("assured_shot_once_a_round", assured_shot_once_a_round),
    ("scene_shots_follow_clip", scene_shots_follow_clip),
    ("assured_shots_follow_clip", assured_shots_follow_clip),
    ("scene_b6a_hands", scene_b6a_hands),
    ("assured_blow_answered", assured_blow_answered),
    ("round_clips_by_wield", round_clips_by_wield),
    ("round_pace", round_pace),
    ("start_refused_held", start_refused_held),
    ("made_stand_apart", made_stand_apart),
    ("scripted_death_in_place", scripted_death_in_place),
    ("held_duel_spacing", held_duel_spacing),
    ("scene_attacks_at_once", scene_attacks_at_once),
    ("duel_closes_to_spacing", duel_closes_to_spacing),
    ("solo_click_holds", solo_click_holds),
    ("coop_private_free", coop_private_free),
    ("private_talk_kotor_blows", private_talk_kotor_blows),
    ("solo_no_gather", solo_no_gather),
    ("coop_gathers", coop_gathers),
    ("line_start_turns_both", line_start_turns_both),
    ("head_arc_turns_less", head_arc_turns_less),
    ("facing_holds_mid_line", facing_holds_mid_line),
    ("lock_orientation", lock_orientation),
    ("within_arc_no_turn", within_arc_no_turn),
    ("player_head_arc", player_head_arc),
    ("lock_head_turns_fully", lock_head_turns_fully),
    ("locks_cleared", locks_cleared),
    ("animated_cut_no_turn", animated_cut_no_turn),
    ("listener_tag", listener_tag),
    ("listener_tag_missing", listener_tag_missing),
    ("listener_previous_speaker", listener_previous_speaker),
    ("listener_self", listener_self),
    ("far_speaker_always_update", far_speaker_always_update),
    ("same_speaker_to_player", same_speaker_to_player),
    ("replies_offered_turn", replies_offered_turn),
    ("reply_picked_turn", reply_picked_turn),
    ("default_angles", default_angles),
    ("line_by_text", line_by_text),
    ("line_delay_raises", line_delay_raises),
    ("line_delay_short", line_delay_short),
    ("silent_line_least", silent_line_least),
    ("cut_line_zero", cut_line_zero),
    ("cut_voiced_line_waits", cut_voiced_line_waits),
    ("voice_is_base", voice_is_base),
    ("short_voice_text", short_voice_text),
    ("wait_voice_holds", wait_voice_holds),
    ("wait_fade_holds", wait_fade_holds),
    ("wait_camera_clip_holds", wait_camera_clip_holds),
    ("pause_is_immediate", pause_is_immediate),
    ("pause_behind_actions", pause_behind_actions),
    ("pause_cleared_with_its_queue", pause_cleared_with_its_queue),
    ("unskippable_line", unskippable_line),
    ("alien_speech", alien_speech),
    ("play_animation_lengths", play_animation_lengths),
    ("long_animation_once", long_animation_once),
    ("loop_ends_on_walk", loop_ends_on_walk),
    ("wait_for_gestures", wait_for_gestures),
    ("walks_at_kotor_speed", walks_at_kotor_speed),
    ("faces_its_path", faces_its_path),
    ("route_from_doorway_heads_on", route_from_doorway_heads_on),
    ("route_rounds_outside_corner", route_rounds_outside_corner),
    ("route_keeps_doorway_corners", route_keeps_doorway_corners),
    ("arrives_exactly", arrives_exactly),
    ("move_to_object_range", move_to_object_range),
    ("goes_round_a_body", goes_round_a_body),
    ("gives_up_blocked", gives_up_blocked),
    ("bump_rules", bump_rules),
    ("pushes_idle_aside", pushes_idle_aside),
    ("immobile_not_pushed", immobile_not_pushed),
    ("body_overlap_not_buried", body_overlap_not_buried),
    ("body_pocket_walked_out", body_pocket_walked_out),
    ("routes_round_made_placeable", routes_round_made_placeable),
    (
        "walker_keeps_off_made_placeable",
        walker_keeps_off_made_placeable,
    ),
    ("free_walk_predicted", free_walk_predicted),
    ("walker_rates", walker_rates),
    ("walk_ends_animation", walk_ends_animation),
    ("forced_jumps_late", forced_jumps_late),
    ("jump_avoids_bodies", jump_avoids_bodies),
    ("free_jump_avoids_bodies", free_jump_avoids_bodies),
    ("jump_beside_close_body", jump_beside_close_body),
    ("jump_mark_on_walkmesh", jump_mark_on_walkmesh),
    ("free_play_unchanged", free_play_unchanged),
    ("ends_mid_walk", ends_mid_walk),
    ("walk_asks_no_exit", walk_asks_no_exit),
    ("leaving_heard", leaving_heard),
    ("item_lost_heard", item_lost_heard),
    ("walk_opens_door", walk_opens_door),
    ("blocked_by_door", blocked_by_door),
    ("dead_open_no_door", dead_open_no_door),
    ("party_stops_at_door", party_stops_at_door),
    ("walk_off_mesh_ends", walk_off_mesh_ends),
    ("door_goal_ends", door_goal_ends),
    ("stepped_walk_stalls", stepped_walk_stalls),
    ("wedged_walk_goes_on_at_once", wedged_walk_goes_on_at_once),
    ("jump_fails_held", jump_fails_held),
    ("free_jump_fails", free_jump_fails),
    ("alternating_walk_stalls", alternating_walk_stalls),
    ("predicted_matches", predicted_matches),
    ("saved_mid_walk", saved_mid_walk),
];

/// A step's length (ms).
const STEP_MS: u64 = 8;
/// How long an answer waits for a line to play (steps): 8 s.
const REPLIES_UP_STEPS: usize = 1000;

/// How many steps last `ms`.
fn steps(ms: u64) -> usize {
    (ms / STEP_MS) as usize
}

/// The ground's door, and the probes' placements after its way out: the
/// one whose conversation the scripts begin, a creature that becomes the
/// host's companion, a hostile brawler and a hostile dummy.
const GATE: u32 = 0;
const TALKER: u32 = 2;
const BUDDY: u32 = 3;
const FOE: u32 = 4;
const DUMMY: u32 = 5;
/// A creature of `Stage::walker`, its body walking at 1.70 m/s and running
/// at 5.40 (`creaturespeed.2da`'s `NORMAL` row).
const WALKER: u32 = 6;

/// The talker's replies: "Again." and "Goodbye." (`dialogue_probes::talking`).
const AGAIN: u8 = 0;
const GOODBYE: u8 = 1;

/// How long a mind that has just spotted its foe takes to act, and a step
/// more (`ai::reaction_ms` with no Awareness).
const AWARE_MS: u64 = 1700;

/// The damage the scene's attack decides (`probe_cutscene`), and one that
/// kills the dummy (`probe_cutscene_kill`).
const SCENE_DAMAGE: i32 = 7;
const KILLING_DAMAGE: i32 = 999;
/// K1's `animations.2da` row of `b7a1`, a rifle's shot (end_room3's
/// soldiers name it).
const SHOT_ROW: u16 = 239;

/// The probes' routines, numbered as `nwscript.nss` would not: only the
/// ones the scripts call.
const CATALOG: &str = r#"
int ATTACK_RESULT_HIT_SUCCESSFUL = 1;
int OBJECT_TYPE_CREATURE = 1;
// 0
int BeginConversation(string sResRef="", object oObjectToDialog=OBJECT_INVALID);
// 1
object GetObjectByTag(string sTag, int nNth=0);
// 2
void ActionAttack(object oAttackee, int bPassive=FALSE);
// 3
object GetFirstPC();
// 4
void CutsceneAttack(object oTarget, int nAnimation, int nAttackResult, int nDamage);
// 5
void ActionOpenDoor(object oDoor);
// 6
void SetFacing(float fDirection);
// 7
void SetLockOrientationInDialog(object oObject, int nValue);
// 8
void SetLockHeadFollowInDialog(object oObject, int nValue);
// 9
void ActionPauseConversation();
// 10
void ActionWait(float fSeconds);
// 11
void ActionResumeConversation();
// 12
void ActionPlayAnimation(int nAnimation, float fSpeed=1.0, float fDurationSeconds=0.0);
// 13
effect EffectForcePushTargeted(location lCentre, int nIgnoreTestDirectLine=0);
// 14
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration=0.0f);
// 15
location GetLocation(object oObject);
// 16
void StartNewModule(string sModuleName, string sWayPoint="", string sMovie1="", string sMovie2="", string sMovie3="", string sMovie4="", string sMovie5="", string sMovie6="");
// 17
void ActionStartConversation(object oObjectToConverse, string sDialogResRef="", int bPrivateConversation=FALSE, int nConversationType=0, int bIgnoreStartRange=FALSE, string sNameObjectToIgnore1="", string sNameObjectToIgnore2="", string sNameObjectToIgnore3="", string sNameObjectToIgnore4="", string sNameObjectToIgnore5="", string sNameObjectToIgnore6="");
// 18
vector Vector(float x=0.0f, float y=0.0f, float z=0.0f);
// 19
location Location(vector vPosition, float fOrientation);
// 20
object CreateObject(int nObjectType, string sTemplate, location lLocation, int bUseAppearAnimation=FALSE);
// 21
void ClearAllActions();
// 22
object GetEnteringObject();
// 23
void SetGlobalBoolean(string sIdentifier, int nValue);
// 24
object GetModuleItemAcquired();
// 25
int GetIsObjectValid(object oObject);
"#;

/// The globals `probe_acquired` sets: whether its `GetEnteringObject` was
/// the first PC (TSL's acquire scripts' test), and whether
/// `GetModuleItemAcquired` named an item (K1's).
const ACQUIRED_BY_PC: &str = "probe_acquired_by_pc";
const ACQUIRED_ITEM: &str = "probe_acquired_item";

/// The `ANIMATION_…` constants the probes play: a looping talk, a looping
/// sleep, a bow and a victory (fire and forget).
const TALK: u16 = 5;
const SLEEP: u16 = 22;
const BOW: u16 = 105;
const VICTORY: u16 = 108;
/// How long the talker's model plays its bow, its talk and its sleep (ms);
/// it has no victory.
const BOW_MS: u32 = 1400;
const TALK_MS: u32 = 2000;
const SLEEP_MS: u32 = 2400;

/// `ops` and a return, as a program.
fn program(ops: &[Op]) -> Ncs {
    let mut asm = script_ncs::probes::Asm::default();
    for op in ops {
        asm.op(op.clone());
    }
    asm.op(Op::Retn);
    asm.build()
}

/// The ops leaving `GetObjectByTag(tag)` on the stack.
fn tagged(tag: &str) -> [Op; 3] {
    [
        Op::Const(Const::Int(0)),
        Op::Const(Const::String(tag.as_bytes().to_vec())),
        Op::Action {
            routine: 1,
            args: 2,
        },
    ]
}

/// The probes' scripts and their one conversation, `talk`, named `dialog`:
/// - `probe_begin` begins it (`BeginConversation`): a party conversation
///   with the first PC;
/// - `probe_attack_buddy`, `probe_attack_dummy` and `probe_attack_pc`
///   attack the companion, the dummy and the first PC (`ActionAttack`);
/// - `probe_open` opens the gate (`ActionOpenDoor`);
/// - `probe_cutscene` is a scene's hit on the dummy for `SCENE_DAMAGE`
///   (`CutsceneAttack`), naming no animation; `probe_cutscene_shot` the
///   same naming K1's rifle shot (`SHOT_ROW`), `probe_cutscene_kill` that
///   shot for `KILLING_DAMAGE`, `probe_cutscene_gate` that shot at the gate
///   for nothing;
/// - `probe_push_dummy` pushes the dummy away from the talker
///   (`EffectForcePushTargeted` at its location, applied at once);
/// - `probe_face_zero` turns its caller to 0° (`SetFacing`);
/// - `probe_lock_turn` and `probe_lock_head` put the talker on the
///   conversation's lock-orientation and head-follow lock lists;
/// - `probe_pause` pauses its caller's conversation, waits 3 s and resumes
///   it; `probe_pause_late` waits 2 s first, then pauses, waits 1 s more and
///   resumes; `probe_pause_cleared` pauses, queues the resume and clears
///   its own queue;
/// - `probe_bow_fast` bows at speed 2 and `probe_victory` cheers
///   (`ActionPlayAnimation`, fire and forget), `probe_talk_3s` talks for
///   3 s, `probe_talk_on` waits 1 s and talks on and on (a duration of −1),
///   and `probe_sleep_twice` sleeps for 40 s twice.
fn library(dialog: &str, talk: Dlg, voices: BTreeMap<String, u32>) -> crate::script::Library {
    let call = |routine, args| Op::Action { routine, args };
    let begin = program(&[
        Op::Const(Const::Object(1)),
        Op::Const(Const::String(dialog.as_bytes().to_vec())),
        call(0, 2),
    ]);
    let attack = |target: &[Op]| {
        let mut ops = vec![Op::Const(Const::Int(0))];
        ops.extend_from_slice(target);
        ops.push(call(2, 2));
        program(&ops)
    };
    let mut open = tagged("gate").to_vec();
    open.push(call(5, 1));
    let cutscene_at = |target: &str, animation: u16, damage: i32| {
        let mut ops = vec![
            Op::Const(Const::Int(damage)),
            Op::Const(Const::Int(1)),
            Op::Const(Const::Int(i32::from(animation))),
        ];
        ops.extend(tagged(target));
        ops.push(call(4, 4));
        program(&ops)
    };
    let cutscene = |animation, damage| cutscene_at("dummy", animation, damage);
    let mut push = vec![Op::Const(Const::Float(0.0))];
    push.extend(tagged("dummy"));
    push.push(Op::Const(Const::Int(0)));
    push.extend(tagged("talker"));
    push.extend([
        call(15, 1),
        call(13, 2),
        Op::Const(Const::Int(0)),
        call(14, 4),
    ]);
    let lock = |routine| {
        let mut ops = vec![Op::Const(Const::Int(1))];
        ops.extend(tagged("talker"));
        ops.push(call(routine, 2));
        program(&ops)
    };
    let face_zero = program(&[Op::Const(Const::Float(0.0)), call(6, 1)]);
    let mut leave: Vec<Op> = (0..7)
        .map(|_| Op::Const(Const::String(Vec::new())))
        .collect();
    leave.push(Op::Const(Const::String(b"elsewhere".to_vec())));
    leave.push(call(16, 8));
    let leave = program(&leave);
    let wait = |seconds| [Op::Const(Const::Float(seconds)), call(10, 1)];
    let pause = program(&[&[call(9, 0)][..], &wait(3.0), &[call(11, 0)]].concat());
    let pause_late = program(&[&wait(2.0)[..], &[call(9, 0)], &wait(1.0), &[call(11, 0)]].concat());
    let pause_cleared = program(&[call(9, 0), call(11, 0), call(21, 0)]);
    let play = |animation: u16, speed, seconds| {
        [
            Op::Const(Const::Float(seconds)),
            Op::Const(Const::Float(speed)),
            Op::Const(Const::Int(i32::from(animation))),
            call(12, 3),
        ]
    };
    let sleep = play(SLEEP, 1.0, 40.0);
    // `ActionStartConversation(GetFirstPC(), dialog)`, a door's locked line.
    let mut converse: Vec<Op> = (0..6)
        .map(|_| Op::Const(Const::String(Vec::new())))
        .collect();
    converse.extend((0..3).map(|_| Op::Const(Const::Int(0))));
    converse.push(Op::Const(Const::String(dialog.as_bytes().to_vec())));
    converse.push(call(3, 0));
    converse.push(call(17, 11));
    let converse = program(&converse);
    // Two `probe_twin`s made at `TWINS_AT` (`CreateObject`).
    let [x, y, z] = floor_at(TWINS_AT);
    let make = [
        Op::Const(Const::Int(0)),
        Op::Const(Const::Float(0.0)),
        Op::Const(Const::Float(z)),
        Op::Const(Const::Float(y)),
        Op::Const(Const::Float(x)),
        call(18, 3),
        call(19, 2),
        Op::Const(Const::String(b"probe_twin".to_vec())),
        Op::Const(Const::Int(1)),
        call(20, 4),
        Op::MovSp(-4),
    ];
    let twins = program(&[make.clone(), make].concat());
    let dialogues: BTreeMap<String, Dlg> = [(dialog.to_string(), talk)].into();
    // K1's `creaturespeed.2da` rows `PLAYER` and `DEFAULT`.
    let rates = crate::script::MoveRates {
        player: Some((3.2, 5.4)),
        default: Some((1.7, 5.4)),
    };
    crate::probe::library(
        CATALOG,
        [
            ("probe_begin", begin),
            ("probe_attack_buddy", attack(&tagged("buddy"))),
            ("probe_attack_dummy", attack(&tagged("dummy"))),
            ("probe_attack_foe", attack(&tagged("foe"))),
            ("probe_attack_pc", attack(&[call(3, 0)])),
            ("probe_open", program(&open)),
            ("probe_blocked", program(&[])),
            ("probe_area_exit", program(&[])),
            (
                "probe_acquired",
                program(&[
                    call(22, 0),
                    call(3, 0),
                    Op::Equal(fmt_odyssey::ncs::Pair::ObjectObject),
                    Op::Const(Const::String(ACQUIRED_BY_PC.as_bytes().to_vec())),
                    call(23, 2),
                    call(24, 0),
                    call(25, 1),
                    Op::Const(Const::String(ACQUIRED_ITEM.as_bytes().to_vec())),
                    call(23, 2),
                ]),
            ),
            ("probe_lost", program(&[])),
            ("probe_module_leave", program(&[])),
            ("probe_leave", leave),
            ("probe_converse", converse),
            ("probe_twins", twins),
            ("probe_cutscene", cutscene(0, SCENE_DAMAGE)),
            ("probe_cutscene_shot", cutscene(SHOT_ROW, SCENE_DAMAGE)),
            ("probe_cutscene_kill", cutscene(SHOT_ROW, KILLING_DAMAGE)),
            ("probe_cutscene_gate", cutscene_at("gate", SHOT_ROW, 0)),
            ("probe_push_dummy", program(&push)),
            ("probe_face_zero", face_zero),
            ("probe_lock_turn", lock(7)),
            ("probe_lock_head", lock(8)),
            ("probe_pause", pause),
            ("probe_pause_late", pause_late),
            ("probe_pause_cleared", pause_cleared),
            ("probe_bow_fast", program(&play(BOW, 2.0, 0.0))),
            ("probe_victory", program(&play(VICTORY, 1.0, 0.0))),
            ("probe_talk_3s", program(&play(TALK, 1.0, 3.0))),
            (
                "probe_talk_on",
                program(&[&wait(1.0)[..], &play(TALK, 1.0, -1.0)].concat()),
            ),
            (
                "probe_sleep_twice",
                program(&[sleep.clone(), sleep].concat()),
            ),
        ],
    )
    .with_dialogues(dialogues, voices)
    .with_move_rates(rates)
    .with_creatures(
        [(
            "probe_twin".to_string(),
            Arc::new(creature("twin", Sheet::commoner(), "")),
        )]
        .into(),
    )
}

/// A point of the ground's floor on its own axes (metres from the
/// doorway's middle, `determinism::ground_point`).
fn floor_at([x, y]: [f32; 2]) -> [f32; 3] {
    crate::determinism::ground_point(x, y, 0.0)
}

/// Where the talker, the buddy, the foe and the dummy wait, a metre apart
/// as the area's loader stands bodies (`install::settle`), and the walker.
const WAITING: [[f32; 2]; 4] = [[-6.0, -5.0], [-5.0, -5.0], [-7.0, -5.0], [-4.0, -5.0]];
const WALKER_WAITS: [f32; 2] = [-3.0, -5.0];

/// A creature of the probes, tagged and named `tag`, playing `sheet`.
fn creature(tag: &str, sheet: Sheet, conversation: &str) -> ObjectData {
    ObjectData {
        kind: ObjectKind::Creature,
        tag: tag.into(),
        name: tag.into(),
        useable: true,
        sheet: Some(sheet),
        conversation: conversation.into(),
        ..ObjectData::unknown("", floor_at([-7.0, -5.0]), 0.0)
    }
}

/// A body whose head turns `arc` degrees from it (`Body::head_arc_h`).
fn body(arc: f32) -> Body {
    Body {
        appearance: 0,
        size: 3,
        movement: 7,
        walk_mps: 1.7,
        run_mps: 5.4,
        head_arc_h: arc,
        bag: 0,
        destroy_ms: 0,
        fade_ms: None,
    }
}

/// What a probe changes of the ground: the talker's DLG and its name, the head arc of
/// its body, that of the host's persona's appearance (row 0 of the
/// library's `head_arcs`; none where 0), the lengths (ms) of the voices its
/// lines name, the lengths of its body's clips (`Library::clip_ms`, its
/// body appearance 0 where it has any) and the `dialoganimations.2da` rows'
/// animations, whether a creature that walks (`WALKER`) joins it, whether
/// the gate stands open leading out of the module, the mesh over its
/// doorway, and whether the mesh goes on through the doorway to the far
/// half, the gate shut, and whether a strip of mesh runs on east through
/// the wall north of the doorway (a walkmesh under a wall's foot, which
/// move_body's box does not pass).
struct Stage {
    talk: Dlg,
    dialog: &'static str,
    arc: f32,
    pc_arc: f32,
    voices: BTreeMap<String, u32>,
    clips: BTreeMap<(u16, u16), u32>,
    /// The camera models' clip lengths (`Library::with_camera_clips`).
    camera_clips: BTreeMap<(String, String), u32>,
    gestures: Vec<Option<u16>>,
    /// TSL's alien speech (`Library::with_alien_voices`).
    aliens: Vec<Vec<String>>,
    walker: bool,
    way_out: bool,
    through: bool,
    over_wall: bool,
    /// The companion's attacks are assured (`EffectAssuredHit`).
    assured: bool,
    /// When the bolts of the guns' attack clips leave the gun
    /// (`Library::discharge`).
    discharges: BTreeMap<(u16, u16), crate::script::effects::Discharge>,
    /// The companion fights with its fists (`Sheet::commoner`'s), not a gun.
    brawling: bool,
    /// The brawler fights with a lightsaber (`Sheet::dev_jedi`).
    jedi: bool,
    /// The companion holds its blaster pistol in each hand (`combat::pair`).
    two_pistols: bool,
    /// The area's `OnExit` and the module's `Mod_OnClientLeav`,
    /// `Mod_OnAcquirItem` and `Mod_OnUnAqreItem` name `probe_area_exit`,
    /// `probe_module_leave`, `probe_acquired` and `probe_lost`.
    leaving: bool,
}

impl Default for Stage {
    fn default() -> Stage {
        Stage {
            talk: crate::script::dialogue_probes::talking(2, ""),
            dialog: "probe_talk",
            arc: 0.0,
            pc_arc: 0.0,
            voices: BTreeMap::new(),
            clips: BTreeMap::new(),
            camera_clips: BTreeMap::new(),
            gestures: Vec::new(),
            aliens: Vec::new(),
            walker: false,
            way_out: false,
            through: false,
            over_wall: false,
            assured: false,
            discharges: BTreeMap::new(),
            brawling: false,
            two_pistols: false,
            jedi: false,
            leaving: false,
        }
    }
}

/// A sheet that outlasts the probes' fights.
fn sturdy(sheet: Sheet) -> Sheet {
    Sheet {
        max_hp: 500,
        ..sheet
    }
}

/// The world a probe runs and what it keeps between steps.
struct Ground {
    world: SimWorld,
    tick: u64,
    seats: u8,
    /// Each seat's answer byte, held as a client holds it.
    replies: Vec<u8>,
    last: Snapshot,
    /// The authority's steps, or a client's predicting (`predicted`).
    reason: StepReason,
}

impl Ground {
    /// The proving ground with the probes' placements and a navigation
    /// mesh over the half players arrive in, `seats` players of their own,
    /// and the brawler made the host's companion: the party's, led by
    /// seat 0, its mind its own.
    fn new(seats: u8) -> Result<Ground, String> {
        Ground::staged(seats, Stage::default())
    }

    /// `new`, the talker staged as `stage` has it.
    fn staged(seats: u8, stage: Stage) -> Result<Ground, String> {
        let ground = crate::determinism::proving_ground();
        let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
        if data.placements.len() != TALKER as usize {
            return Err(format!("the ground has {} objects", data.placements.len()));
        }
        let brawler = if stage.jedi {
            Sheet {
                faction: HOSTILE_1,
                ..Sheet::dev_jedi()
            }
        } else {
            Sheet {
                abilities: [16, 14, 14, 10, 10, 10],
                faction: HOSTILE_1,
                ..Sheet::commoner()
            }
        };
        data.placements.extend(
            [
                ObjectData {
                    body: (stage.arc > 0.0 || !stage.clips.is_empty()).then(|| body(stage.arc)),
                    ..creature("talker", Sheet::commoner(), stage.dialog)
                },
                creature(
                    "buddy",
                    sturdy(Sheet {
                        flags: if stage.assured {
                            crate::combat::sheet::flags::ASSURED_HIT
                        } else {
                            0
                        },
                        ..if stage.brawling {
                            Sheet::commoner()
                        } else if stage.two_pistols {
                            let mut pair = Sheet::dev_soldier();
                            pair.weapons[1] = pair.weapons[0];
                            pair
                        } else {
                            Sheet::dev_soldier()
                        }
                    }),
                    "",
                ),
                creature("foe", sturdy(brawler), ""),
                creature(
                    "dummy",
                    sturdy(Sheet {
                        faction: HOSTILE_1,
                        ..Sheet::commoner()
                    }),
                    "",
                ),
            ]
            .into_iter()
            .zip(WAITING)
            .map(|(made, at)| {
                Placement::Own(ObjectData {
                    position: floor_at(at),
                    ..made
                })
            }),
        );
        if stage.walker {
            data.placements.push(Placement::Own(ObjectData {
                position: floor_at(WALKER_WAITS),
                body: Some(body(0.0)),
                scripts: vec![("ScriptOnBlocked".into(), "probe_blocked".into())],
                ..creature("walker", Sheet::commoner(), "")
            }));
        }
        data.repute = Repute::at_war([PLAYER, HOSTILE_1]);
        if stage.leaving {
            data.area_scripts = vec![("OnExit".into(), "probe_area_exit".into())];
            data.module_scripts = [
                ("Mod_OnClientLeav", "probe_module_leave"),
                ("Mod_OnAcquirItem", "probe_acquired"),
                ("Mod_OnUnAqreItem", "probe_lost"),
            ]
            .map(|(hook, script)| (hook.to_string(), script.to_string()))
            .to_vec();
        }
        if stage.way_out
            && let Some(Placement::Own(gate)) = data.placements.get_mut(GATE as usize)
        {
            gate.link = Some(crate::module::Link {
                module: "elsewhere".into(),
                tag: String::new(),
                destination: None,
            });
            gate.state = 1;
        }
        let quad = |[x0, y0, x1, y1]: [f32; 4]| {
            let [a, b, c, d] = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(floor_at);
            [[a, b, c], [a, c, d]]
        };
        let mut floor = quad([-7.5, -5.5, -0.5, 5.5]).to_vec();
        if stage.way_out || stage.through {
            floor.extend(quad([-0.5, -1.0, 0.5, 1.0]));
        }
        if stage.through {
            floor.extend(quad([0.5, -5.5, 3.5, 5.5]));
        }
        if stage.over_wall {
            floor.extend(quad([-0.5, 2.0, 2.0, 5.5]));
        }
        let mut nav = collision::nav::NavMesh::build(&floor, crate::walk::CLEARANCE * UNIT);
        if let Some(level) = ground.collision().level() {
            nav.mark(level);
        }
        let level = ground
            .with_module(data)
            .with_scripts(
                library(stage.dialog, stage.talk, stage.voices)
                    .with_head_arcs(vec![stage.pc_arc])
                    .with_clips(stage.clips)
                    .with_camera_clips(stage.camera_clips)
                    .with_dialog_animations(stage.gestures)
                    .with_alien_voices(stage.aliens)
                    .with_discharges(stage.discharges)
                    .with_rounds(probe_rounds()),
            )
            .with_nav(nav);
        let config = WorldConfig {
            seed: 1,
            seats,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: sturdy(Sheet::dev_soldier()),
            guests: crate::Guests::Avatars,
        };
        let mut world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
        let last = world.snapshot();
        let mut ground = Ground {
            world,
            tick: 0,
            seats,
            replies: vec![0; usize::from(seats)],
            last,
            reason: StepReason::Authority,
        };
        let buddy = ground.object(BUDDY)?;
        let mut fighters = ground.world.world.query::<(&Id, &mut Fighting)>();
        for (id, mut fighting) in fighters.iter_mut(&mut ground.world.world) {
            if id.0 == buddy {
                fighting.0.mind.leader = Some(Seat(0));
                fighting.0.faction = Some(PLAYER);
            }
        }
        if stage.pc_arc > 0.0 {
            ground.world.set_persona(crate::persona::Persona {
                seat: Seat(0),
                appearance: 0,
                gender: crate::persona::MALE,
                portrait: 0,
                name: String::new(),
            });
        }
        ground.step();
        Ok(ground)
    }

    /// The entity placement `index` stands as.
    fn object(&self, index: u32) -> Result<SimId, String> {
        self.last
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, crate::Kind::Object(_, of) if of == index))
            .map(|entity| entity.id)
            .ok_or(format!("no object {index}"))
    }

    fn body(&self, seat: u8) -> Result<SimId, String> {
        self.last
            .body(Seat(seat))
            .map(|entity| entity.id)
            .ok_or(format!("seat {seat} has no body"))
    }

    /// One step with `input`'s actions, each seat standing still with its
    /// held answer byte.
    fn step_with(&mut self, mut input: TickInput) {
        self.tick += 1;
        for seat in 0..self.seats {
            let reply = self.replies[usize::from(seat)];
            input.insert(
                Seat(seat),
                Command {
                    reply,
                    ..Command::default()
                },
            );
        }
        self.last = crate::step(
            &mut self.world,
            self.tick,
            &input,
            STEP_MS as u32,
            self.reason,
        );
    }

    fn step(&mut self) {
        self.step_with(TickInput::default());
    }

    fn idle(&mut self, ms: u64) {
        for _ in 0..steps(ms) {
            self.step();
        }
    }

    /// `who` standing still at `at` on the ground's floor.
    fn put(&mut self, who: SimId, at: [f32; 2]) {
        let p = floor_at(at);
        let mut players = self.world.world.query::<(&Id, &mut Player)>();
        for (id, mut player) in players.iter_mut(&mut self.world.world) {
            if id.0 == who {
                let ps = &mut player.0;
                ps.origin = [
                    p[0] / UNIT,
                    p[1] / UNIT,
                    p[2] / UNIT + 0.125 - crate::motion::SOLE_Z,
                ];
                ps.velocity = [0.0; 3];
            }
        }
    }

    /// Placement `index` stands still at `at`.
    fn place(&mut self, index: u32, at: [f32; 2]) -> Result<SimId, String> {
        let who = self.object(index)?;
        self.put(who, at);
        Ok(who)
    }

    /// Placement `caller` runs `script`, in the next step.
    fn run_script(&mut self, script: &str, caller: u32) -> Result<(), String> {
        let caller = self.object(caller)?;
        let mut state = self.world.script_state().clone();
        state
            .pending
            .push(crate::script::Pending::new(script, ObjectId(caller.0)));
        self.world.set_script_state(state);
        self.step();
        Ok(())
    }

    /// A script of the talker begins its party conversation.
    fn cutscene(&mut self) -> Result<(), String> {
        self.run_script("probe_begin", TALKER)?;
        match self.last.talk.party() {
            Some(_) => Ok(()),
            None => Err("the script began no party conversation".into()),
        }
    }

    /// Seat `seat`'s body clicks the talker (a player's own conversation).
    fn click_talker(&mut self, seat: u8) -> Result<(), String> {
        let mut input = TickInput::default();
        let verb = Verb::Talk;
        input.act(
            Seat(seat),
            Action::Use {
                object: TALKER,
                verb,
            },
        );
        self.step_with(input);
        let body = ObjectId(self.body(seat)?.0);
        match self.last.talk.holding(body) {
            Some(_) => Ok(()),
            None => Err(format!("seat {seat}'s click began no conversation")),
        }
    }

    /// Every conversation ends: each seat answers "Goodbye.", a few times
    /// at most.
    fn goodbye(&mut self) -> Result<(), String> {
        for _ in 0..4 {
            if self.last.talk.conversations.is_empty() {
                return Ok(());
            }
            self.replies_up();
            for reply in &mut self.replies {
                *reply = answer(*reply, Pick::Reply(GOODBYE));
            }
            self.step();
        }
        Err("the conversation did not end".into())
    }

    /// Whether a conversation holds the world (`scene::held`), the seats'
    /// bodies as the minds collect them.
    fn held(&mut self) -> bool {
        let lent = super::super::lent(&self.world.script_state().party);
        let mut avatars = self.world.world.query::<(&Id, &Avatar)>();
        let seats: Vec<(Seat, SimId)> = avatars
            .iter(&self.world.world)
            .map(|(id, avatar)| (avatar.0, id.0))
            .collect();
        let bodies = super::seat_bodies(seats, lent);
        super::held(self.world.world.resource::<Talking>(), &bodies)
    }

    fn entity(&self, id: SimId) -> Option<&crate::EntityState> {
        self.last.entities.iter().find(|entity| entity.id == id)
    }

    fn at(&self, id: SimId) -> [f32; 3] {
        self.entity(id).map_or([0.0; 3], |entity| entity.position)
    }

    /// `id`'s hit points, whole.
    fn hp(&self, id: SimId) -> i32 {
        self.entity(id)
            .and_then(|entity| entity.fighter)
            .map_or(0, |fighter| fighter.hp / 100)
    }

    /// Where `id` faces (degrees from +X).
    fn yaw(&self, id: SimId) -> f32 {
        self.entity(id)
            .and_then(|entity| entity.player)
            .map_or(f32::NAN, |ps| ps.view[1])
    }

    /// How far (degrees) `who` faces from `toward`.
    fn off(&self, who: SimId, toward: SimId) -> f32 {
        let (a, b) = (self.at(who), self.at(toward));
        apart(
            self.yaw(who),
            crate::math::atan2_deg(b[1] - a[1], b[0] - a[0]),
        )
    }

    /// `who` turned to `yaw` (degrees), as a script turns it.
    fn turn(&mut self, who: SimId, yaw: f32) {
        let mut players = self.world.world.query::<(&Id, &mut Player)>();
        for (id, mut player) in players.iter_mut(&mut self.world.world) {
            if id.0 == who {
                player.0.face(yaw);
            }
        }
    }

    /// Seat 0 answers `pick`, a step; a reply once the replies are up.
    fn answer(&mut self, pick: Pick) {
        if matches!(pick, Pick::Reply(_) | Pick::Point(_)) {
            self.replies_up();
        }
        self.replies[0] = answer(self.replies[0], pick);
        self.step();
    }

    /// Steps until no line under way is still playing: its replies are
    /// chosen as it ends (`Step::offer_replies`), and an answer counts from
    /// then.
    fn replies_up(&mut self) {
        for _ in 0..REPLIES_UP_STEPS {
            let playing = self
                .last
                .talk
                .conversations
                .iter()
                .any(|c| c.until_ms != u64::MAX && !c.on_hold());
            if !playing {
                return;
            }
            self.step();
        }
    }

    /// The line under way: the entry shown and its listener.
    fn line(&self) -> Option<(u32, ObjectId)> {
        let c = self.last.talk.conversations.first()?;
        Some((c.entry, c.listener_now))
    }

    /// Steps until entry `entry` shows, for 3 s at most, then a step more.
    fn until_entry(&mut self, entry: u32) -> Result<(), String> {
        for _ in 0..steps(3000) {
            self.step();
            if self.line().is_some_and(|(shown, _)| shown == entry) {
                self.step();
                return Ok(());
            }
        }
        Err(format!("entry {entry} never showed: {:?}", self.line()))
    }

    fn gate_shut(&self) -> bool {
        self.last
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, crate::Kind::Object(_, GATE)))
            .and_then(|entity| entity.object)
            .is_some_and(|object| object.state == 0)
    }

    /// Over `ms`, `who`'s strafe and how much nearer `to` it came (m). The
    /// strafe is the speed its command's sideways key moves it at (its
    /// speed by the key's share of the axes), a mean over the window; the
    /// drift how far its body moved sideways to its facing each step while
    /// it closed in, until it first came within its reach (`mind::REACH`),
    /// a mean over those steps (both m/s). The drift is what a wandering
    /// aim turns a run aside by; the slide along the body it then meets is
    /// left out. A step first shows where the bodies were put.
    fn strafe(&mut self, who: SimId, to: SimId, ms: u64) -> Strafe {
        self.step();
        let before = between(self.at(who), self.at(to));
        let reach = crate::combat::mind::REACH * UNIT;
        let (mut keyed, mut drift, mut closing) = (0.0, 0.0, 0);
        let mut reached = false;
        for _ in 0..steps(ms) {
            let from = self.at(who);
            reached |= between(from, self.at(to)) <= reach;
            self.step();
            let Some(entity) = self.entity(who) else {
                continue;
            };
            let [vx, vy, _] = entity.velocity;
            let right = entity
                .fighter
                .map_or(0, |fighter| fighter.last_command.right);
            keyed += vx.hypot(vy) * f32::from(right).abs() / 127.0;
            let yaw = entity.player.map_or(0.0, |ps| ps.view[1]);
            let (sin, cos) = crate::math::sin_cos_deg(yaw);
            if !reached {
                let [dx, dy, _] = crate::math::sub(entity.position, from);
                drift += (dy * cos - dx * sin).abs();
                closing += 1;
            }
        }
        let window = steps(ms).max(1) as f32;
        Strafe {
            keyed: keyed / window,
            drift: drift / (closing.max(1) as f32 * STEP_MS as f32 / 1000.0),
            closed: before - between(self.at(who), self.at(to)),
        }
    }
}

/// How far apart two points are on the floor (m).
fn between(a: [f32; 3], b: [f32; 3]) -> f32 {
    crate::math::flat_distance(a, b)
}

/// How far apart two headings are (degrees, 0 to 180).
fn apart(a: f32, b: f32) -> f32 {
    let off = (a - b).rem_euclid(360.0);
    off.min(360.0 - off)
}

/// A fighter's strafe over a window (`Ground::strafe`).
struct Strafe {
    keyed: f32,
    drift: f32,
    closed: f32,
}

impl std::fmt::Display for Strafe {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "strafe {:.3} m/s (drift {:.3} m/s, closed {:.2} m)",
            self.keyed, self.drift, self.closed
        )
    }
}

/// The most a scene's fighter strafes (m/s, mean over the window).
const STRAFE_MAX: f32 = 0.1;
/// The most a held fighter drifts as it closes in (m/s, `Ground::strafe`):
/// KOTOR's fighters don't wander. JA's wandering aim drifts one 0.156 m/s,
/// a steady aim 0.012.
const DRIFT_MAX: f32 = 0.05;

/// Where the companion stands near the host, and where its foe comes from
/// (6 m off).
const HOST_AT: [f32; 2] = [-6.0, -1.5];
const BUDDY_AT: [f32; 2] = [-7.0, 2.0];
const FOE_FROM: [f32; 2] = [-1.0, 2.0];
const TALKER_AT: [f32; 2] = [-6.0, -4.0];

/// The brawler attacking `target` (`probe_attack_…`) from 6 m once it is
/// aware of it: its strafe over 2 s.
fn charge(ground: &mut Ground, script: &str, target: SimId) -> Result<Strafe, String> {
    ground.run_script(script, FOE)?;
    ground.idle(AWARE_MS);
    let foe = ground.place(FOE, FOE_FROM)?;
    ground.put(target, BUDDY_AT);
    Ok(ground.strafe(foe, target, 2000))
}

/// S4: a party conversation up, a hostile brawler attacking the companion
/// (not in it) closes in and holds its ground with no strafe, its aim not
/// wandering its run aside; once it ends, the brawler bears off to the side
/// as JA's do.
fn bystander_holds_ground() -> Result<String, String> {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, HOST_AT);
    ground.place(TALKER, TALKER_AT)?;
    let buddy = ground.place(BUDDY, BUDDY_AT)?;
    ground.cutscene()?;
    let held = charge(&mut ground, "probe_attack_buddy", buddy)?;
    ground.goodbye()?;
    let free = charge(&mut ground, "probe_attack_buddy", buddy)?;
    check(
        held.keyed < STRAFE_MAX
            && held.drift < DRIFT_MAX
            && held.closed > 2.0
            && free.keyed > STRAFE_MAX,
        format!("held: {held}; after the end: {free}"),
    )
}

/// S4: the companion 8 m from its leader stays put through a party
/// conversation and follows once it ends.
fn companion_stays() -> Result<String, String> {
    const FAR: [f32; 2] = [-1.5, 2.6];
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -4.0]);
    ground.place(TALKER, [-7.0, -3.0])?;
    let buddy = ground.place(BUDDY, FAR)?;
    ground.step();
    let apart = between(ground.at(host), ground.at(buddy));
    ground.cutscene()?;
    let start = ground.at(buddy);
    ground.idle(3000);
    let moved = between(start, ground.at(buddy));
    ground.goodbye()?;
    ground.idle(3000);
    let came = apart - between(ground.at(host), ground.at(buddy));
    check(
        (7.5..8.5).contains(&apart) && moved < 0.05 && came > 1.0,
        format!("{apart:.2} m off: moved {moved:.3} m held, then came {came:.2} m nearer"),
    )
}

/// S4: the host's use of the gate while a party conversation is up fails
/// at once, the gate shut; once it ends, the same use opens it.
fn party_cannot_use() -> Result<String, String> {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, [-1.2, 0.0]);
    ground.place(TALKER, [-3.0, -1.0])?;
    ground.cutscene()?;
    let open = |ground: &mut Ground| {
        let mut input = TickInput::default();
        input.act(
            Seat(0),
            Action::Use {
                object: GATE,
                verb: Verb::Open,
            },
        );
        ground.step_with(input);
        ground.idle(200);
        ground.gate_shut()
    };
    let held = open(&mut ground);
    ground.goodbye()?;
    let after = open(&mut ground);
    check(
        held && !after,
        format!("shut after the use held {held}, after the end {after}"),
    )
}

/// S4: the companion's queued `ActionOpenDoor` while a party conversation
/// is up ends at once, the gate shut; once it ends, the same opens it.
fn party_queue_cannot_open() -> Result<String, String> {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, [-3.0, 1.0]);
    ground.place(TALKER, [-3.0, -1.0])?;
    let buddy = ground.place(BUDDY, [-1.2, 0.8])?;
    ground.cutscene()?;
    ground.run_script("probe_open", BUDDY)?;
    ground.idle(1000);
    let shut = ground.gate_shut();
    let queued = ground
        .world
        .script_state()
        .actions
        .get(&buddy.0)
        .map_or(0, Vec::len);
    ground.goodbye()?;
    ground.run_script("probe_open", BUDDY)?;
    ground.idle(1000);
    let after = ground.gate_shut();
    check(
        shut && queued == 0 && !after,
        format!("held: shut {shut}, queued {queued}; after the end shut {after}"),
    )
}

/// The companion, a gunner, beside the dummy it attacks.
const GUNNER_AT: [f32; 2] = [-5.0, 2.0];
const DUMMY_AT: [f32; 2] = [-2.0, 2.0];

/// S4: the companion attacking the dummy (`ActionAttack`) through a party
/// conversation lands nothing; once it ends, its shots do.
fn party_rolls_nothing() -> Result<String, String> {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    ground.place(TALKER, [-7.0, -3.0])?;
    ground.place(BUDDY, GUNNER_AT)?;
    let dummy = ground.place(DUMMY, DUMMY_AT)?;
    ground.cutscene()?;
    let full = ground.hp(dummy);
    ground.run_script("probe_attack_dummy", BUDDY)?;
    ground.idle(4000);
    let held = full - ground.hp(dummy);
    ground.goodbye()?;
    ground.run_script("probe_attack_dummy", BUDDY)?;
    ground.idle(4000);
    let after = full - ground.hp(dummy) - held;
    check(
        held == 0 && after > 0,
        format!("the dummy lost {held} HP held, {after} after the end"),
    )
}

/// S4: a scene's attack (`CutsceneAttack`) by the companion, a gunner,
/// through a party conversation: it fires its shot, and the dummy takes
/// the scripted damage.
fn cutscene_attack_lands() -> Result<String, String> {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    ground.place(TALKER, [-7.0, -3.0])?;
    let buddy = ground.place(BUDDY, GUNNER_AT)?;
    let dummy = ground.place(DUMMY, DUMMY_AT)?;
    ground.cutscene()?;
    let full = ground.hp(dummy);
    ground.run_script("probe_cutscene", BUDDY)?;
    let mut fired = false;
    for _ in 0..steps(3000) {
        ground.step();
        fired |= ground
            .entity(buddy)
            .and_then(|entity| entity.fighter)
            .is_some_and(|fighter| fighter.last_command.buttons & buttons::ATTACK != 0);
    }
    let lost = full - ground.hp(dummy);
    let up = ground.last.talk.party().is_some();
    check(
        fired && lost == SCENE_DAMAGE && up,
        format!("fired {fired}, the dummy lost {lost} HP, the conversation up {up}"),
    )
}

/// A scene's melee blow (`CutsceneAttack` by the brawler, beside the
/// dummy) lands with its weapon in hand: the dummy's hit event carries the
/// brawler's fists, as a fight's blow does (the views' hit sparks, blood
/// and sound read it).
fn cutscene_blow_weapon() -> Result<String, String> {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    ground.place(TALKER, [-7.0, -3.0])?;
    let dummy = ground.place(DUMMY, DUMMY_AT)?;
    ground.place(FOE, FOE_FROM)?;
    ground.cutscene()?;
    let full = ground.hp(dummy);
    ground.run_script("probe_cutscene", FOE)?;
    ground.idle(4000);
    let lost = full - ground.hp(dummy);
    let weapon = ground
        .entity(dummy)
        .and_then(|entity| entity.fighter)
        .and_then(|fighter| {
            fighter
                .newest()
                .find(|event| event.damage > 0)
                .map(|e| e.weapon)
        });
    check(
        lost == SCENE_DAMAGE && weapon == Some(Some(crate::combat::Delivery::Unarmed)),
        format!("the dummy lost {lost} HP, struck with {weapon:?}"),
    )
}

/// A scene's shot (`probe_cutscene_shot`, the companion a gunner, the dummy
/// 3 m off), from the step the script runs: when (ms) the companion begins
/// its shot's animation and the dummy its reaction (its acting, with when
/// it began), and when the dummy lost its HP.
struct Shot {
    swing: Option<(u64, Acting)>,
    reaction: Option<(u64, Acting)>,
    struck: Option<u64>,
    lost: i32,
}

fn shot(script: &str, stage: Stage) -> Result<(Ground, SimId, Shot), String> {
    let mut ground = Ground::staged(1, stage)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    ground.place(TALKER, [-7.0, -3.0])?;
    let buddy = ground.place(BUDDY, GUNNER_AT)?;
    let dummy = ground.place(DUMMY, DUMMY_AT)?;
    ground.cutscene()?;
    let full = ground.hp(dummy);
    ground.run_script(script, BUDDY)?;
    let acting = |ground: &Ground, who: SimId| ground.entity(who)?.fighter?.acting;
    let mut seen = Shot {
        swing: None,
        reaction: None,
        struck: None,
        lost: 0,
    };
    for step in 1..=steps(2500) {
        ground.step();
        let ms = step as u64 * STEP_MS;
        if seen.swing.is_none() {
            seen.swing = acting(&ground, buddy).map(|acting| (ms, acting));
        }
        if seen.reaction.is_none() {
            seen.reaction = acting(&ground, dummy).map(|acting| (ms, acting));
        }
        if seen.struck.is_none() && ground.hp(dummy) < full {
            seen.struck = Some(ms);
        }
    }
    seen.lost = full - ground.hp(dummy);
    Ok((ground, dummy, seen))
}

/// A scene's shot (`CutsceneAttack` naming `b7a1`) by a gunner: it plays the
/// row from the round's start, and its bolt lands the blow half a round
/// in, as the blow falls due (not before), the target playing the table's
/// answer to a hit (`blows::HURT` and its row) from the moment it is
/// struck, as a melee blow's target does.
fn cutscene_shot_rounds() -> Found {
    use crate::script::effects::blows;
    let (_, _, seen) = shot("probe_cutscene_shot", Stage::default())?;
    let half = crate::script::actions::ROUND_MS / 2;
    let swung = seen.swing.is_some_and(|(ms, acting)| {
        ms <= 2 * STEP_MS && acting.animation == blows::SWING + SHOT_ROW
    });
    let landed = seen.struck.is_some_and(|ms| {
        (half - STEP_MS..=half + crate::script::effects::SHOT_GRACE_MS).contains(&ms)
    });
    let answered = seen
        .reaction
        .zip(seen.struck)
        .is_some_and(|((ms, acting), struck)| {
            ms == struck && acting.animation == blows::HURT + PROBE_DAMAGE
        });
    check(
        swung && landed && answered && seen.lost == SCENE_DAMAGE,
        format!(
            "swing {:?}, reaction {:?}, struck at {:?} ms (due at {half}), lost {} HP",
            seen.swing.map(|(ms, a)| (ms, a.animation)),
            seen.reaction.map(|(ms, a)| (ms, a.animation)),
            seen.struck,
            seen.lost
        ),
    )
}

/// A scene's shot at a door or placeable (`CutsceneAttack` on the Endar
/// Spire's fuel cylinder, which the script then blows up): the gunner turns
/// to it and fires its bolt there, as at a character.
fn cutscene_shot_at_object() -> Found {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    ground.place(TALKER, [-7.0, -3.0])?;
    let buddy = ground.place(BUDDY, GUNNER_AT)?;
    let gate = ground.object(GATE)?;
    ground.cutscene()?;
    ground.run_script("probe_cutscene_gate", BUDDY)?;
    let mut fired = None;
    for step in 1..=steps(2000) {
        ground.step();
        let bolt = ground
            .last
            .entities
            .iter()
            .any(|entity| matches!(entity.kind, crate::Kind::Missile));
        if bolt && fired.is_none() {
            fired = Some((step as u64 * STEP_MS, ground.off(buddy, gate)));
        }
    }
    check(
        fired.is_some_and(|(_, off)| off <= 10.0),
        format!("its bolt (ms, degrees off the gate): {fired:?}"),
    )
}

/// A script's push outside a cast (`EffectForcePushTargeted`: the Endar
/// Spire's blast throwing a soldier) knocks its target down and throws it
/// away from the centre.
fn script_push_throws() -> Found {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    let talker = ground.place(TALKER, [-5.0, 2.0])?;
    let dummy = ground.place(DUMMY, [-4.0, 2.0])?;
    ground.cutscene()?;
    let before = between(ground.at(talker), ground.at(dummy));
    ground.run_script("probe_push_dummy", TALKER)?;
    let mut down = false;
    for _ in 0..steps(500) {
        ground.step();
        let mut fighters = ground.world.world.query::<(&Id, &Fighting)>();
        down |= fighters
            .iter(&ground.world.world)
            .any(|(id, fighting)| id.0 == dummy && fighting.0.saber.floored());
    }
    let after = between(ground.at(talker), ground.at(dummy));
    check(
        down && after > before + 1.0,
        format!("knocked down {down}, {before:.2} m from the centre, then {after:.2} m"),
    )
}

/// A gunner's assured attack (`EffectAssuredHit`, then `ActionAttack`: Taris's
/// duel, Gerlon's shot) goes out as its shot as a scene's does: its round's
/// clip (`b5a1`, a pistol's first) from the round's start, the bolt
/// landing the blow half a round in, the target struck then, answering
/// with the table's damage clip.
fn assured_shot_rounds() -> Found {
    use crate::script::effects::blows;
    let stage = Stage {
        assured: true,
        ..Stage::default()
    };
    let (_, _, seen) = shot("probe_attack_dummy", stage)?;
    let row = seen
        .swing
        .map_or(0, |(_, acting)| acting.animation % blows::ROWS);
    let shot_row = row == B5A1;
    let answered = seen
        .reaction
        .zip(seen.struck)
        .is_some_and(|((ms, acting), struck)| {
            ms == struck && acting.animation == blows::HURT + PROBE_DAMAGE
        });
    let half = crate::script::actions::ROUND_MS / 2;
    let landed = seen.struck.is_some_and(|ms| {
        (half - STEP_MS..=half + 4 * STEP_MS + crate::script::effects::SHOT_GRACE_MS).contains(&ms)
    });
    check(
        shot_row && answered && landed && seen.lost > 0,
        format!(
            "swing {:?}, reaction {:?}, struck at {:?} ms, lost {} HP",
            seen.swing.map(|(ms, a)| (ms, a.animation)),
            seen.reaction.map(|(ms, a)| (ms, a.animation)),
            seen.struck,
            seen.lost
        ),
    )
}

/// A brawler's assured attack (`EffectAssuredHit`, then `ActionAttack`
/// beside its target: Taris's gang scene, Bek and Vulkar trading blows)
/// plays as the round's attack, as K1's executable plays it: its bare
/// hands' swing (`g8a1` or `g8a2`), the target answering the hit with the
/// table's damage clip, and the blow landing.
fn assured_blow_answered() -> Found {
    use crate::script::effects::blows;
    let stage = Stage {
        assured: true,
        brawling: true,
        ..Stage::default()
    };
    let mut ground = Ground::staged(1, stage)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    ground.place(TALKER, [-7.0, -3.0])?;
    let buddy = ground.place(BUDDY, [DUMMY_AT[0] - 1.0, DUMMY_AT[1]])?;
    let dummy = ground.place(DUMMY, DUMMY_AT)?;
    ground.cutscene()?;
    let full = ground.hp(dummy);
    ground.run_script("probe_attack_dummy", BUDDY)?;
    let acting = |ground: &Ground, who: SimId| ground.entity(who)?.fighter?.acting;
    let (mut swing, mut reaction) = (None, None);
    for _ in 0..steps(2500) {
        ground.step();
        let kind = |who| acting(&ground, who).and_then(|a| blows::of(a.animation));
        swing = swing.or(kind(buddy).filter(|(kind, _)| *kind == blows::SWING));
        reaction = reaction.or(kind(dummy).filter(|(kind, _)| *kind == blows::HURT));
    }
    let lost = full - ground.hp(dummy);
    let fists = swing.is_some_and(|(_, row)| [G8A1, G8A1 + 1].contains(&row));
    check(
        fists && reaction.map(|(_, row)| row) == Some(PROBE_DAMAGE) && lost > 0,
        format!("swing {swing:?}, reaction {reaction:?}, lost {lost} HP"),
    )
}

/// The rows of the probes' round table (`probe_rounds`): K1's own
/// numbers for the clips a round swings.
const B5A1: u16 = 217;
const G1A1: u16 = 87;
const C2A1: u16 = 94;
const F2A3: u16 = 115;
const G2A1: u16 = 122;
const G8A1: u16 = 247;
/// Every swing's answers in the probes' table: a parry, a dodge and a hit
/// (K1's `g2r2`, `g2g1`, `g2d1`).
const PROBE_PARRY: u16 = 134;
const PROBE_DODGE: u16 = 131;
const PROBE_DAMAGE: u16 = 124;

/// A round table made for the probes: the rows above by name, each
/// answered alike by every class, its hit at K1's `hit1` (a gun's none).
fn probe_rounds() -> crate::combat::rounds::Table {
    use crate::combat::rounds::{Answers, CLASSES, NONE, Table};
    let mut rows = std::collections::BTreeMap::new();
    let mut answers = std::collections::BTreeMap::new();
    let mut add = |name: String, row: u16, hit_ms: u16| {
        rows.insert(name, row);
        let cells = [[PROBE_PARRY, PROBE_DODGE, PROBE_DAMAGE]; CLASSES];
        answers.insert(row, Answers { hit_ms, cells });
    };
    for (name, row, hit_ms) in [
        ("g1a1", G1A1, 500),
        ("g1a2", G1A1 + 1, 500),
        ("f2a3", F2A3, 867),
        ("g2a1", G2A1, 500),
        ("g2a2", G2A1 + 1, 500),
        ("g8a1", G8A1, 500),
        ("g8a2", G8A1 + 1, 500),
        ("b5a1", B5A1, NONE),
        ("b6a1", B6A1, NONE),
        ("b7a1", SHOT_ROW, NONE),
    ] {
        add(name.to_string(), row, hit_ms);
    }
    for (variant, hit_ms) in [1267, 900, 1167, 750, 867].into_iter().enumerate() {
        add(format!("c2a{}", variant + 1), C2A1 + variant as u16, hit_ms);
    }
    Table { rows, answers }
}

/// K1's executable's round clips (`combat::rounds`): a stun baton swings
/// `g1a1` or `g1a2` at anyone, never a cinematic set (K1 has no `c1a*`);
/// a blade at a baton plain `g2a*`; blade against blade the cinematic
/// `c2a1` to `c2a5`; a baton's Power Attack `g1a1` and a blade's `f2a3`;
/// bare hands `g8a*`. The target answers a hit with the table's damage
/// clip, a miss with a parry holding a weapon and a dodge bare-handed, and
/// the blow lands at the swing's `hit1`. One holding a heavy repeater (class
/// 9, past the table's columns) answers with nothing, not a panic.
fn round_clips_by_wield() -> Found {
    use crate::combat::Delivery;
    use crate::combat::rounds::{Answer, Side};
    use crate::combat::sheet::{Weapon, feats};
    let table = probe_rounds();
    let armed = |delivery: Delivery| Sheet {
        weapons: [
            Some(Weapon {
                delivery,
                ..Weapon::UNARMED
            }),
            None,
        ],
        ..Sheet::commoner()
    };
    let (baton, blade, bare) = (
        armed(Delivery::Baton),
        armed(Delivery::Melee),
        Sheet::commoner(),
    );
    let side = |sheet| Side {
        sheet,
        creature: false,
    };
    let swings = |by, target, feat| -> Vec<Option<u16>> {
        (1..=5)
            .map(|variant| table.swing((side(by), side(target)), feat, |count| variant.min(count)))
            .collect()
    };
    let rows = |rows: &[u16]| rows.iter().map(|row| Some(*row)).collect::<Vec<_>>();
    let power = Some(feats::POWER_ATTACK[0]);
    let cases = [
        (
            "baton at blade",
            swings(&baton, &blade, None),
            rows(&[G1A1, G1A1 + 1, G1A1 + 1, G1A1 + 1, G1A1 + 1]),
        ),
        (
            "blade at baton",
            swings(&blade, &baton, None),
            rows(&[G2A1, G2A1 + 1, G2A1 + 1, G2A1 + 1, G2A1 + 1]),
        ),
        (
            "blade at blade",
            swings(&blade, &blade, None),
            rows(&[C2A1, C2A1 + 1, C2A1 + 2, C2A1 + 3, C2A1 + 4]),
        ),
        (
            "baton power",
            swings(&baton, &blade, power),
            rows(&[G1A1; 5]),
        ),
        (
            "blade power",
            swings(&blade, &blade, power),
            rows(&[F2A3; 5]),
        ),
        (
            "bare hands",
            swings(&bare, &blade, None),
            rows(&[G8A1, G8A1 + 1, G8A1 + 1, G8A1 + 1, G8A1 + 1]),
        ),
    ];
    let wrong: Vec<_> = cases
        .iter()
        .filter(|(_, got, want)| got != want)
        .map(|(name, got, _)| format!("{name}: {got:?}"))
        .collect();
    let answer = |landed, target| table.answer(C2A1, Answer::of_round(landed, target), target);
    let heavy = Sheet {
        weapons: [
            Some(Weapon {
                delivery: Delivery::Blaster(crate::combat::sheet::Blaster::Repeater),
                base_item: 24,
                ..Weapon::UNARMED
            }),
            None,
        ],
        ..Sheet::commoner()
    };
    let answers = [
        answer(true, side(&blade)),
        answer(false, side(&baton)),
        answer(false, side(&bare)),
        answer(true, side(&heavy)),
    ];
    // K1's classes with a weapon in each hand go by the left one's.
    let pair = |right: Delivery, left: Delivery| {
        let mut sheet = armed(right);
        sheet.weapons[1] = armed(left).weapons[0];
        sheet.wield_number()
    };
    let classes = [
        pair(Delivery::Baton, Delivery::Melee),
        pair(Delivery::Melee, Delivery::Baton),
        bare.wield_number(),
    ];
    let hits = [table.hit_ms(C2A1), table.hit_ms(G1A1), table.hit_ms(B5A1)];
    check(
        wrong.is_empty()
            && answers
                == [
                    Some(PROBE_DAMAGE),
                    Some(PROBE_PARRY),
                    Some(PROBE_DODGE),
                    None,
                ]
            && hits == [Some(1267), Some(500), None]
            && classes == [4, 0, 8],
        format!("{wrong:?}; answers {answers:?}; hits {hits:?}; classes {classes:?}"),
    )
}

/// A creature's rounds come a round apart (`ROUND_MS`), one attack a
/// round, as K1's executable's round holds one attack action a side: the
/// brawler's assured blows on the dummy (the gang scene's), which does not
/// fight back, come once a round, not again as each blow lands, though its
/// script asks for the attack every second.
fn round_pace() -> Found {
    use crate::script::effects::blows;
    let stage = Stage {
        assured: true,
        brawling: true,
        ..Stage::default()
    };
    let mut ground = Ground::staged(1, stage)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    ground.place(TALKER, [-7.0, -3.0])?;
    let buddy = ground.place(BUDDY, [DUMMY_AT[0] - 1.0, DUMMY_AT[1]])?;
    ground.place(DUMMY, DUMMY_AT)?;
    ground.cutscene()?;
    let mut swings: Vec<u64> = Vec::new();
    for step in 0..steps(8000) {
        // Its script asks for the attack again every second.
        if step % steps(1000) == 0 {
            ground.run_script("probe_attack_dummy", BUDDY)?;
        }
        ground.step();
        let swing = ground
            .entity(buddy)
            .and_then(|e| e.fighter?.acting)
            .filter(|a| blows::of(a.animation).is_some_and(|(kind, _)| kind == blows::SWING));
        if let Some(acting) = swing
            && swings.last() != Some(&acting.since)
        {
            swings.push(acting.since);
        }
    }
    let gaps: Vec<u64> = swings.windows(2).map(|w| w[1] - w[0]).collect();
    check(
        swings.len() >= 2
            && gaps
                .iter()
                .all(|gap| *gap + STEP_MS >= crate::combat::ROUND_MS),
        format!("swings at {swings:?} ms, gaps {gaps:?}"),
    )
}

/// `ActionStartConversation` while a conversation holds the world is
/// refused, as both games' executables refuse it (their dialog state set:
/// a locked door's line, its `OnFailToOpen` run as a scene walks someone
/// into it, is never said): the companion queues nothing, where in free
/// play it queues its conversation.
fn start_refused_held() -> Found {
    let queued = |held: bool| -> Result<bool, String> {
        let mut ground = Ground::new(1)?;
        let host = ground.body(0)?;
        ground.put(host, [-6.0, -3.0]);
        ground.place(TALKER, [-7.0, -3.0])?;
        let buddy = ground.place(BUDDY, GUNNER_AT)?;
        if held {
            ground.cutscene()?;
        }
        ground.run_script("probe_converse", BUDDY)?;
        let state = ground.world.script_state();
        Ok(state.actions.get(&buddy.0).is_some_and(|queue| {
            queue.iter().any(|queued| {
                matches!(
                    queued.action,
                    crate::script::actions::Action::Converse { .. }
                )
            })
        }))
    };
    let (free, held) = (queued(false)?, queued(true)?);
    check(
        free && !held,
        format!("queued its conversation: free play {free}, held {held}"),
    )
}

/// Where `probe_twins` makes its two creatures (the ground's axes).
const TWINS_AT: [f32; 2] = [-3.0, 0.0];

/// Two creatures a script makes on one spot (`CreateObject`; stunt_07's
/// Bastila and Carth, `SpawnAvailableNPC` at one waypoint) stand where the
/// safe-location search puts them, as a jump's place: the first on the
/// spot, the second clear of it by both personal spaces (0.7 m).
fn made_stand_apart() -> Found {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    let before: Vec<SimId> = ground.last.entities.iter().map(|e| e.id).collect();
    ground.run_script("probe_twins", TALKER)?;
    ground.step();
    let made: Vec<[f32; 3]> = ground
        .last
        .entities
        .iter()
        .filter(|e| !before.contains(&e.id) && e.player.is_some())
        .map(|e| e.position)
        .collect();
    let [first, second] = made[..] else {
        return Err(format!("made {} creatures", made.len()));
    };
    let (on_spot, apart) = (between(first, floor_at(TWINS_AT)), between(first, second));
    check(
        on_spot <= 0.05 && apart >= 0.69,
        format!("the first {on_spot:.2} m from the spot, the two {apart:.2} m apart"),
    )
}

/// A gunner's assured attack is its round's one attack: its shot struck,
/// it shoots no more before the attack ends with the round (Taris's duel:
/// Gerlon's one shot, after which Duncan lies in the dead pose his
/// surrender plays, never shot to his feet again).
fn assured_shot_once_a_round() -> Found {
    let mut ground = Ground::staged(
        1,
        Stage {
            assured: true,
            ..Stage::default()
        },
    )?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    ground.place(TALKER, [-7.0, -3.0])?;
    ground.place(BUDDY, GUNNER_AT)?;
    let dummy = ground.place(DUMMY, DUMMY_AT)?;
    ground.cutscene()?;
    ground.run_script("probe_attack_dummy", BUDDY)?;
    let mut hp = ground.hp(dummy);
    let mut struck = Vec::new();
    for step in 1..=steps(2 * crate::script::actions::ROUND_MS) {
        ground.step();
        let now = ground.hp(dummy);
        if now < hp {
            struck.push(step as u64 * STEP_MS);
        }
        hp = now;
    }
    check(
        struck.len() == 1,
        format!("struck at {struck:?} ms over two rounds"),
    )
}

/// K1's `weapondischarge.2da`: when (ms into the clip) `b7a1`'s three bolts
/// leave a rifle, `b5a1`'s two a pistol.
const B7A1_SHOTS: [u16; 3] = [200, 633, 1200];
const B5A1_SHOTS: [u16; 2] = [467, 967];

/// A gunner's shots as the probes see them: when (ms from the step the
/// script ran) its clip began, each bolt it fired left (`Missile::from_ms`,
/// as offsets from the clip's start), and the target lost its HP, and how
/// much it lost.
struct Volley {
    clip: Option<u64>,
    bolts: Vec<u64>,
    /// Each bolt left the gunner's left side.
    left: Vec<bool>,
    struck: Option<u64>,
    lost: i32,
}

/// The volley `script` fires from the companion on the dummy, the stage's
/// clips given `discharges` and the companion's attacks `assured` or not.
fn volley(
    script: &str,
    assured: bool,
    discharges: BTreeMap<(u16, u16), crate::script::effects::Discharge>,
) -> Result<Volley, String> {
    volley_on(
        script,
        Stage {
            assured,
            discharges,
            ..Stage::default()
        },
    )
}

/// The volley `script` fires from the companion on the dummy on `stage`.
fn volley_on(script: &str, stage: Stage) -> Result<Volley, String> {
    let mut ground = Ground::staged(1, stage)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.0, -3.0]);
    ground.place(TALKER, [-7.0, -3.0])?;
    let buddy = ground.place(BUDDY, GUNNER_AT)?;
    let dummy = ground.place(DUMMY, DUMMY_AT)?;
    ground.cutscene()?;
    let full = ground.hp(dummy);
    ground.run_script(script, BUDDY)?;
    let start = ground.last.time_ms - STEP_MS;
    let mut seen = Volley {
        clip: None,
        bolts: Vec::new(),
        left: Vec::new(),
        struck: None,
        lost: 0,
    };
    let mut fired = std::collections::BTreeSet::new();
    for _ in 0..steps(2 * crate::script::actions::ROUND_MS) {
        ground.step();
        let acting = ground.entity(buddy).and_then(|e| e.fighter?.acting);
        if seen.clip.is_none() {
            seen.clip = acting.map(|acting| acting.since - start);
        }
        for entity in &ground.last.entities {
            if let Some(bolt) = entity.missile.filter(|m| m.owner == buddy)
                && fired.insert(entity.id)
            {
                seen.bolts.push(bolt.from_ms - start);
                let ps = ground.entity(buddy).and_then(|e| e.player);
                seen.left.push(ps.is_some_and(|ps| {
                    let (_, right, _) = crate::math::angle_vectors(ps.view);
                    let off = crate::math::sub(bolt.base, crate::combat::mind::eye(&ps));
                    crate::math::dot(off, right) < 0.0
                }));
            }
        }
        if seen.struck.is_none() && ground.hp(dummy) < full {
            seen.struck = Some(ground.last.time_ms - start);
        }
    }
    seen.lost = full - ground.hp(dummy);
    Ok(seen)
}

/// Whether `seen` fired `shots` (ms into its clip), each within a step of
/// its moment, and was struck once, after the last left.
fn follows(seen: &Volley, shots: &[u16]) -> bool {
    let clip = seen.clip.unwrap_or(u64::MAX / 2);
    let on_time = seen.bolts.len() == shots.len()
        && seen
            .bolts
            .iter()
            .zip(shots)
            .all(|(bolt, ms)| bolt.abs_diff(clip + u64::from(*ms)) <= STEP_MS);
    let last = seen.bolts.last().copied().unwrap_or(u64::MAX);
    on_time && seen.struck.is_some_and(|ms| ms > last) && seen.lost > 0
}

fn shown(seen: &Volley) -> String {
    let clip = seen.clip.unwrap_or(0);
    format!(
        "clip at {:?} ms, bolts at {:?} ms into it, struck at {:?} ms into it, lost {} HP",
        seen.clip,
        seen.bolts
            .iter()
            .map(|b| b.saturating_sub(clip))
            .collect::<Vec<_>>(),
        seen.struck.map(|s| s.saturating_sub(clip)),
        seen.lost
    )
}

/// A scene's shot (`CutsceneAttack` naming `b7a1`) fires the clip's bolts
/// when the clip fires them (`weapondischarge.2da`: 200, 633 and 1200 ms
/// in), not one bolt half a round on: the first leaves with the clip's first
/// kick, and the last carries the blow, the target struck once.
fn scene_shots_follow_clip() -> Found {
    use crate::script::effects::Discharge;
    let table = [(
        (crate::script::ANY_BODY, SHOT_ROW),
        Discharge::new(&B7A1_SHOTS),
    )]
    .into();
    let seen = volley("probe_cutscene_shot", false, table)?;
    check(
        follows(&seen, &B7A1_SHOTS) && seen.lost == SCENE_DAMAGE,
        shown(&seen),
    )
}

/// A gunner's assured attack (Taris's duel: Gerlon's `b5a1`, a pistol's
/// first shot clip) fires its clip's two bolts at 467 and 967 ms in, the
/// second landing the blow.
fn assured_shots_follow_clip() -> Found {
    use crate::script::effects::Discharge;
    let table = [((crate::script::ANY_BODY, B5A1), Discharge::new(&B5A1_SHOTS))].into();
    let seen = volley("probe_attack_dummy", true, table)?;
    check(follows(&seen, &B5A1_SHOTS), shown(&seen))
}

/// K1's `weapondischarge.2da` row 231, `b6a1` (two pistols' first shot
/// clip): its bolts' times and `switchmask`.
const B6A1: u16 = 231;
const B6A1_SHOTS: [u16; 4] = [233, 400, 833, 1000];
const B6A1_MASK: &str = "1010";

/// A gunner with a pistol in each hand, its attack assured, plays `b6a1`
/// and fires each bolt when the clip does from the hand its `switchmask`
/// names (`1` the left): left, right, left, right.
fn scene_b6a_hands() -> Found {
    use crate::script::effects::Discharge;
    let shots = Discharge::of(&B6A1_SHOTS, Discharge::mask(B6A1_MASK));
    let seen = volley_on(
        "probe_attack_dummy",
        Stage {
            assured: true,
            two_pistols: true,
            discharges: [((crate::script::ANY_BODY, B6A1), shots)].into(),
            ..Stage::default()
        },
    )?;
    check(
        follows(&seen, &B6A1_SHOTS) && seen.left == [true, false, true, false],
        format!("{}; left {:?}", shown(&seen), seen.left),
    )
}

/// A creature killed by a scene's shot plays KOTOR's dying clip where it
/// stands (`death::in_place`): its acting the clip from its death, its
/// body unpushed, still a second on; and keeps it, the dead acting on
/// nothing: an animation a script then plays on it (queued, and at once)
/// does not take its dying clip's place (half a second on, before the
/// engine takes the body away).
fn scripted_death_in_place() -> Found {
    let (mut ground, dummy, seen) = shot("probe_cutscene_kill", Stage::default())?;
    let at = ground.at(dummy);
    ground.idle(1000);
    let moved = between(at, ground.at(dummy));
    let acting = |ground: &Ground| ground.entity(dummy).and_then(|e| e.fighter?.acting);
    let died = acting(&ground);
    let dead = ground.hp(dummy) <= 0;
    ground.run_script("probe_victory", DUMMY)?;
    ground.run_script("probe_bow_fast", DUMMY)?;
    ground.idle(500);
    let after = acting(&ground);
    let dying = |acting: Option<crate::combat::idle::Acting>| {
        acting.is_some_and(|acting| acting.animation == crate::combat::death::DIE)
    };
    check(
        dead && dying(died) && after == died && moved < 0.01 && seen.struck.is_some(),
        format!(
            "dead {dead}, acting {:?}, moved {moved:.3} m after its death; scripts' animations after it: acting {:?}",
            died.map(|acting| acting.animation),
            after.map(|acting| acting.animation)
        ),
    )
}

/// S4: in a held scene the brawler attacking the companion closes to the
/// spacing KOTOR's round clips are made for and stands there: its centre
/// `DUEL_SPACING_M` from its target's (`appearance.2da` `prefatckdist`,
/// centre to centre; the paired `c2a`/`c2p` clips cross their blades with
/// the bodies 1.25 to 1.3 m apart).
fn held_duel_spacing() -> Found {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, HOST_AT);
    ground.place(TALKER, TALKER_AT)?;
    let buddy = ground.place(BUDDY, BUDDY_AT)?;
    ground.cutscene()?;
    charge(&mut ground, "probe_attack_buddy", buddy)?;
    let foe = ground.object(FOE)?;
    let mut nearest = f32::MAX;
    for _ in 0..steps(2000) {
        ground.step();
        nearest = nearest.min(between(ground.at(foe), ground.at(buddy)));
    }
    let apart = between(ground.at(foe), ground.at(buddy));
    check(
        (apart - DUEL_SPACING_M).abs() < 0.08 && nearest > DUEL_SPACING_M - 0.1,
        format!("{apart:.2} m apart after closing in, {nearest:.2} m at the nearest"),
    )
}

/// S4: two of a scene's blade fighters running at each other (the duel of
/// end_cut04: KOTOR rounds, `ActionAttack` between creatures) both stop at
/// `DUEL_SPACING_M`, each letting go of its run as far before it as both
/// slide.
fn duel_closes_to_spacing() -> Found {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, HOST_AT);
    ground.place(TALKER, TALKER_AT)?;
    let buddy = ground.place(BUDDY, BUDDY_AT)?;
    ground.cutscene()?;
    ground.run_script("probe_attack_buddy", FOE)?;
    ground.run_script("probe_attack_foe", BUDDY)?;
    ground.idle(AWARE_MS);
    let foe = ground.place(FOE, FOE_FROM)?;
    ground.put(buddy, BUDDY_AT);
    let mut nearest = f32::MAX;
    for _ in 0..steps(3000) {
        ground.step();
        nearest = nearest.min(between(ground.at(foe), ground.at(buddy)));
    }
    let apart = between(ground.at(foe), ground.at(buddy));
    check(
        (apart - DUEL_SPACING_M).abs() < 0.12 && nearest > DUEL_SPACING_M - 0.15,
        format!("{apart:.2} m apart after closing in, {nearest:.2} m at the nearest"),
    )
}

/// S4, KOTOR's rounds: in a held scene a saber wielder told to attack the
/// companion beside it (`ActionAttack`) swings at once, with no moment of
/// awareness, and keeps swinging, never holding off to guard. Over 3 s:
/// when (ms) it first pressed attack, and the share of the steps after
/// that it held attack.
fn scene_attacks_at_once() -> Found {
    let stage = Stage {
        jedi: true,
        ..Stage::default()
    };
    let mut ground = Ground::staged(1, stage)?;
    let host = ground.body(0)?;
    ground.put(host, HOST_AT);
    ground.place(TALKER, TALKER_AT)?;
    ground.place(BUDDY, BUDDY_AT)?;
    let foe = ground.place(FOE, [BUDDY_AT[0] + 1.4, BUDDY_AT[1]])?;
    ground.cutscene()?;
    ground.run_script("probe_attack_buddy", FOE)?;
    let (mut first, mut pressed, mut after) = (None, 0, 0);
    for step in 1..=steps(3000) {
        ground.step();
        let attacking = ground
            .entity(foe)
            .and_then(|entity| entity.fighter)
            .is_some_and(|fighter| fighter.last_command.buttons & buttons::ATTACK != 0);
        if attacking && first.is_none() {
            first = Some(step as u64 * STEP_MS);
        }
        if first.is_some() {
            after += 1;
            pressed += usize::from(attacking);
        }
    }
    let share = pressed as f32 / after.max(1) as f32;
    check(
        first.is_some_and(|ms| ms < AWARE_MS / 2) && share > 0.9,
        format!("first pressed at {first:?} ms, held {share:.2} of the steps since"),
    )
}

/// How far apart (m, centre to centre) a held scene's fighters stand.
const DUEL_SPACING_M: f32 = 1.4;

/// S1: one seat, the player clicks the talker (its own conversation, not a
/// party one): it holds the world.
fn solo_click_holds() -> Result<String, String> {
    let mut ground = Ground::new(1)?;
    let host = ground.body(0)?;
    ground.put(host, [-5.0, 0.0]);
    ground.place(TALKER, [-4.0, 0.0])?;
    ground.click_talker(0)?;
    let party = ground.last.talk.party().is_some();
    let held = ground.held();
    check(!party && held, format!("a party one {party}, held {held}"))
}

/// S1: two seats, the guest in its own conversation: the world is not
/// held, and the host's foe bears off to the side as JA's do.
fn coop_private_free() -> Result<String, String> {
    let mut ground = Ground::new(2)?;
    let (host, guest) = (ground.body(0)?, ground.body(1)?);
    ground.put(guest, [-3.0, -4.0]);
    ground.place(TALKER, [-2.0, -4.0])?;
    ground.click_talker(1)?;
    let held = ground.held();
    let strafe = charge(&mut ground, "probe_attack_pc", host)?;
    let up = ground.last.talk.holding(ObjectId(guest.0)).is_some();
    check(
        !held && up && strafe.keyed > STRAFE_MAX,
        format!("held {held}, the guest's up {up}, {strafe}"),
    )
}

/// A hit from `by` on `on` this step, as the fights and the scripts leave
/// them for `hurt`.
fn strike(ground: &mut Ground, by: SimId, on: SimId, source: crate::combat::damage::Source) {
    let hit = super::super::Hit {
        attacker: by,
        target: on,
        source,
        point: None,
        direction: Some([1.0, 0.0, 0.0]),
    };
    let world = &mut ground.world.world;
    world.resource_mut::<super::super::Hits>().0.push(hit);
    ground.step();
}

/// S1, one scene gate (`Held::scene`): two seats, the guest in its own
/// conversation with the talker (the world not held). The dummy's pushing
/// power pushes the guest not, as KOTOR pushes no one in a conversation,
/// and the host, in none, it pushes; a script's sting makes the talker
/// flinch not, and the companion it does (unaware of a foe, a flinch is
/// certain); a power's killing `EffectDamage` (a cast's) on the talker
/// plays KOTOR's dying clip where it stands.
fn private_talk_kotor_blows() -> Found {
    use crate::combat::damage::Source;
    let mut ground = Ground::new(2)?;
    let (host, guest) = (ground.body(0)?, ground.body(1)?);
    ground.put(host, [-6.0, 3.0]);
    ground.put(guest, [-3.0, -4.0]);
    let talker = ground.place(TALKER, [-2.0, -4.0])?;
    let buddy = ground.place(BUDDY, [-6.0, 0.0])?;
    let dummy = ground.place(DUMMY, [-4.0, 4.0])?;
    ground.click_talker(1)?;
    let held = ground.held();
    let push = Source::Power {
        points: 20,
        extra: 0,
        area: false,
        pushes: true,
        damage_type: None,
    };
    let sting = |cast, damage| Source::Script {
        damage,
        damage_type: None,
        cast,
        shot: None,
        staged: false,
    };
    let speed = |ground: &Ground, who: SimId| {
        ground
            .entity(who)
            .map_or(0.0, |entity| entity.velocity[0].hypot(entity.velocity[1]))
    };
    let pain = |ground: &Ground, who: SimId| {
        ground
            .entity(who)
            .and_then(|entity| entity.fighter)
            .is_some_and(|fighter| fighter.pain_until > ground.last.time_ms)
    };
    // A blow on the guest's side may end its conversation: it talks again
    // before the next.
    let talking = |ground: &mut Ground| match ground.last.talk.holding(ObjectId(guest.0)) {
        Some(_) => Ok(()),
        None => ground.click_talker(1),
    };
    let mut found = Vec::new();
    for (on, source) in [(guest, push), (host, push)] {
        talking(&mut ground)?;
        strike(&mut ground, dummy, on, source);
        found.push(speed(&ground, on));
    }
    let mut flinched = Vec::new();
    for on in [talker, buddy] {
        talking(&mut ground)?;
        strike(&mut ground, dummy, on, sting(false, 1));
        flinched.push(pain(&ground, on));
    }
    talking(&mut ground)?;
    strike(&mut ground, dummy, talker, sting(true, 999));
    let acting = ground.entity(talker).and_then(|e| e.fighter?.acting);
    let in_place = acting.is_some_and(|acting| acting.animation == crate::combat::death::DIE);
    check(
        !held && found[0] < 0.01 && found[1] > 0.5 && flinched == [false, true] && in_place,
        format!(
            "held {held}; pushed to (guest, host) {found:.2?} m/s; flinched (talker, \
             companion) {flinched:?}; the talker's death {:?}",
            acting.map(|acting| acting.animation)
        ),
    )
}

/// The companion 12 m from the host, a party conversation begun: how far
/// it then stands from the host (m), and from where it stood before.
fn gathered(seats: u8) -> Result<(f32, f32), String> {
    let mut ground = Ground::new(seats)?;
    let host = ground.body(0)?;
    ground.put(host, [-6.5, -5.2]);
    for seat in 1..seats {
        let guest = ground.body(seat)?;
        ground.put(guest, [-5.5, -5.2]);
    }
    ground.place(TALKER, [-7.0, -4.0])?;
    let buddy = ground.place(BUDDY, [-0.7, 5.3])?;
    ground.step();
    let before = ground.at(buddy);
    ground.cutscene()?;
    ground.step();
    Ok((
        between(ground.at(host), ground.at(buddy)),
        between(before, ground.at(buddy)),
    ))
}

/// S9: one seat, a party conversation: the companion far off stays where
/// it is.
fn solo_no_gather() -> Result<String, String> {
    let (apart, moved) = gathered(1)?;
    check(
        apart > 11.0 && moved < 0.05,
        format!("{apart:.2} m from the host, moved {moved:.3} m"),
    )
}

/// S9: two seats, a party conversation: the companion far off is brought
/// near the host (I30 P7, as `dialogue_coop`'s `party_talk_gathers`).
fn coop_gathers() -> Result<String, String> {
    let (apart, moved) = gathered(2)?;
    check(
        apart <= 4.0 && moved > 5.0,
        format!("{apart:.2} m from the host, moved {moved:.2} m"),
    )
}

/// Where the facing probes stand, on the ground's axes: the talker, the
/// host 2 m north of it, and the companion 2 m south or east of it.
const FACING_TALKER: [f32; 2] = [-5.0, 0.0];
const FACING_HOST: [f32; 2] = [-5.0, 2.0];
const FACING_SOUTH: [f32; 2] = [-5.0, -2.0];
const FACING_EAST: [f32; 2] = [-3.0, 0.0];

/// The heading (degrees from game space's +X) from `from` to `to`, points
/// on the ground's axes.
fn heading(from: [f32; 2], to: [f32; 2]) -> f32 {
    let (a, b) = (floor_at(from), floor_at(to));
    crate::math::atan2_deg(b[1] - a[1], b[0] - a[0])
}

/// The heading of the ground's +X axis.
fn east() -> f32 {
    heading([0.0, 0.0], [1.0, 0.0])
}

/// One seat on the ground staged as `stage`: the talker, the host and the
/// companion (at `buddy_at`) placed, all facing the ground's +X (`east`),
/// so the talker and the host stand 90° off each other's facing. The
/// talker, the host and the companion.
fn placed(stage: Stage, buddy_at: [f32; 2]) -> Result<(Ground, [SimId; 3]), String> {
    let mut ground = Ground::staged(1, stage)?;
    let host = ground.body(0)?;
    ground.put(host, FACING_HOST);
    let talker = ground.place(TALKER, FACING_TALKER)?;
    let buddy = ground.place(BUDDY, buddy_at)?;
    for who in [talker, host, buddy] {
        ground.turn(who, east());
    }
    Ok((ground, [talker, host, buddy]))
}

/// The talker's party conversation begun, and a step on.
fn begin(ground: &mut Ground) -> Result<(), String> {
    ground.cutscene()?;
    ground.step();
    Ok(())
}

/// `placed`, then `begin`.
fn facing(stage: Stage, buddy_at: [f32; 2]) -> Result<(Ground, [SimId; 3]), String> {
    let (mut ground, ids) = placed(stage, buddy_at)?;
    begin(&mut ground)?;
    Ok((ground, ids))
}

/// The talker's DLG with `change` made to it.
fn talk_with(change: impl FnOnce(&mut Dlg)) -> Stage {
    let mut stage = Stage::default();
    change(&mut stage.talk);
    stage
}

/// Lines said by `speakers` in turn (tags; empty for the talker), none
/// naming a listener, a second each, joined by silent replies; the last
/// line ends it.
fn in_turn(speakers: &[&str]) -> Dlg {
    let mut dlg = crate::script::dialogue_probes::talking(0, "");
    let (entry, silent) = (dlg.entries[0].clone(), dlg.replies[0].clone());
    let link = |index| Link {
        index,
        ..dlg.starts[0].clone()
    };
    let last = speakers.len() as u32 - 1;
    dlg.entries = speakers
        .iter()
        .zip(0..)
        .map(|(speaker, n)| Node {
            speaker: speaker.to_string(),
            delay: Some(1),
            links: if n < last { vec![link(n)] } else { Vec::new() },
            ..entry.clone()
        })
        .collect();
    dlg.replies = (1..=last)
        .map(|next| Node {
            links: vec![link(next)],
            ..silent.clone()
        })
        .collect();
    dlg
}

/// The angles a DLG named `dialog` resolves (S6), six default lines by the
/// talker in turn (`in_turn`), the last leading back to the first by a
/// seventh silent reply: each entry's as it shows, the first shown again
/// last, and the six silent replies'.
fn walked_angles(dialog: &'static str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut talk = in_turn(&[""; 6]);
    let back = Link {
        index: 0,
        ..talk.starts[0].clone()
    };
    talk.entries[5].links = vec![Link {
        index: 5,
        ..back.clone()
    }];
    let mut silent = talk.replies[0].clone();
    silent.links = vec![back];
    talk.replies.push(silent);
    let stage = Stage {
        talk,
        dialog,
        ..Stage::default()
    };
    let mut ground = Ground::staged(1, stage)?;
    ground.cutscene()?;
    let angle = |ground: &Ground| {
        ground
            .last
            .talk
            .conversations
            .first()
            .map_or(0, |c| c.angle)
    };
    let mut entries = vec![angle(&ground)];
    for entry in [1, 2, 3, 4, 5, 0] {
        ground.until_entry(entry)?;
        entries.push(angle(&ground));
    }
    let c = (ground.last.talk.conversations.first()).ok_or("no conversation under way")?;
    let replies = (0..6u16)
        .map(|reply| {
            let at = c
                .angles
                .iter()
                .find(|(node, _)| *node == REPLY_ANGLE | reply);
            at.map_or(0, |(_, angle)| *angle)
        })
        .collect();
    Ok((entries, replies))
}

/// S6, the default angles of `walked_angles`. Each entry shown and each
/// reply set moves the walk once, taking the angle at the place before the
/// move, past 18 back to 0; the first entry and the first set take 2; the
/// first entry shown again keeps its angle. The name's hash is
/// Σ c·(1 + int(10^i)) over its characters, its remainders unsigned:
/// - `end_trask01`, hash 1977608757: start 8, step 3, forward (odd);
/// - `probe_talk`, hash 2932504419 (−1362462877 signed, which would start
///   at 10 and step 4): start 16, step 5, forward. Its first set's place
///   holds a 1, so the set's 2 is the first set's rule.
///
/// | node | `end_trask01` place, table, angle | `probe_talk` place, table, angle |
/// |---|---|---|
/// | entry 0 | 8, 1, 2 (first) | 16, 1, 2 (first) |
/// | replies 0 | 11, 2, 2 (first) | 0, 1, 2 (first) |
/// | entry 1 | 14, 2, 2 | 5, 2, 2 |
/// | replies 1 | 17, 1, 1 | 10, 1, 1 |
/// | entry 2 | 0, 1, 1 | 15, 3, 3 |
/// | replies 2 | 3, 3, 3 | 0, 1, 1 |
/// | entry 3 | 6, 1, 1 | 5, 2, 2 |
/// | replies 3 | 9, 2, 2 | 10, 1, 1 |
/// | entry 4 | 12, 1, 1 | 15, 3, 3 |
/// | replies 4 | 15, 3, 3 | 0, 1, 1 |
/// | entry 5 | 18, 1, 1 | 5, 2, 2 |
/// | replies 5 | 0, 1, 1 | 10, 1, 1 |
/// | entry 0 again | 3, 3, 2 (kept) | 15, 3, 2 (kept) |
fn default_angles() -> Found {
    let cases: [(&'static str, [u8; 7], [u8; 6]); 2] = [
        ("end_trask01", [2, 2, 1, 1, 1, 1, 2], [2, 1, 3, 2, 3, 1]),
        ("probe_talk", [2, 2, 3, 2, 3, 2, 2], [2, 1, 1, 1, 1, 1]),
    ];
    let mut ok = true;
    let mut found = Vec::new();
    for (dialog, entries, replies) in cases {
        let (got_entries, got_replies) = walked_angles(dialog)?;
        ok &= got_entries == entries && got_replies == replies;
        found.push(format!(
            "{dialog}: entries {got_entries:?} (want {entries:?}), replies {got_replies:?} (want \
             {replies:?})"
        ));
    }
    check(ok, found.join("; "))
}

/// The degrees shown in a probe's report.
fn degrees(values: &[f32]) -> String {
    let shown: Vec<String> = values.iter().map(|value| format!("{value:.1}°")).collect();
    shown.join(", ")
}

/// S2: at the line's start the listener (the host) turns to the speaker
/// (the talker), then the speaker to it: with no head arc, both fully.
fn line_start_turns_both() -> Found {
    let (ground, [talker, host, _]) = facing(Stage::default(), FACING_SOUTH)?;
    let offs = [ground.off(host, talker), ground.off(talker, host)];
    let listener = ground.line().map(|(_, listener)| listener);
    check(
        offs.iter().all(|off| *off < 1.0) && listener == Some(ObjectId(host.0)),
        format!(
            "the host and the talker off each other: {}; the listener {listener:?}",
            degrees(&offs)
        ),
    )
}

/// The talker, its head arc 30°, turned `off` degrees from its aim at the
/// host, the line begun: how far its body turned, and how far it then
/// faces from the host (degrees).
fn arc_turn(off: f32) -> Result<[f32; 2], String> {
    let stage = Stage {
        arc: 30.0,
        ..Stage::default()
    };
    let (mut ground, [talker, host, _]) = placed(stage, FACING_SOUTH)?;
    let before = heading(FACING_TALKER, FACING_HOST) + off;
    ground.turn(talker, before);
    begin(&mut ground)?;
    Ok([apart(ground.yaw(talker), before), ground.off(talker, host)])
}

/// S2: with a head arc of 30° and the host 90° to either side, the
/// talker's body turns 59° toward it (θ − (arc + 1°)), the head left the
/// rest.
fn head_arc_turns_less() -> Found {
    let turns = [90.0, -90.0].map(arc_turn);
    let ok = turns.iter().all(|turn| {
        turn.as_ref()
            .is_ok_and(|[turned, left]| (turned - 59.0).abs() < 1.0 && (left - 31.0).abs() < 1.0)
    });
    check(
        ok,
        format!("(turned, left off the host) with the host to either side: {turns:?}"),
    )
}

/// S2: with a head arc of 30° and the host 20° off, the talker's body
/// doesn't turn: the head alone follows.
fn within_arc_no_turn() -> Found {
    let [turned, left] = arc_turn(20.0)?;
    check(
        turned < 1.0 && (left - 20.0).abs() < 1.0,
        format!("the talker turned {turned:.1}°, {left:.1}° off the host"),
    )
}

/// S2: the host's persona's appearance has a head arc of 30°
/// (`Library::head_arc`): its body turns 59° to the talker, 90° off.
fn player_head_arc() -> Found {
    let stage = Stage {
        pc_arc: 30.0,
        ..Stage::default()
    };
    let (ground, [talker, host, _]) = facing(stage, FACING_SOUTH)?;
    let (turned, left) = (apart(ground.yaw(host), east()), ground.off(host, talker));
    check(
        (turned - 59.0).abs() < 1.0 && (left - 31.0).abs() < 1.0,
        format!("the host turned {turned:.1}°, {left:.1}° off the talker"),
    )
}

/// S2: the talker turned to 0° by a script (`SetFacing`) during the line
/// stays so a second on; the next line turns it back to the host.
fn facing_holds_mid_line() -> Found {
    let (mut ground, [talker, host, _]) = facing(Stage::default(), FACING_SOUTH)?;
    let shown = ground.last.talk.conversations.first().map(|c| c.shown_ms);
    ground.run_script("probe_face_zero", TALKER)?;
    ground.idle(1000);
    let held = apart(ground.yaw(talker), 0.0);
    let same = ground.last.talk.conversations.first().map(|c| c.shown_ms) == shown;
    ground.answer(Pick::Reply(AGAIN));
    ground.step();
    let next = ground.off(talker, host);
    check(
        held < 1.0 && same && next < 1.0,
        format!(
            "{held:.1}° off 0° a second on (the same line {same}), {next:.1}° off the host at \
             the next"
        ),
    )
}

/// The talker put on a lock list by `script` during the line, it and the
/// host turned back to `east`, then "Again.": how far the talker turned,
/// and how far the host then faces from it (degrees).
fn relocked(stage: Stage, script: &str) -> Result<(f32, f32), String> {
    let (mut ground, [talker, host, _]) = facing(stage, FACING_SOUTH)?;
    ground.run_script(script, TALKER)?;
    for who in [talker, host] {
        ground.turn(who, east());
    }
    ground.answer(Pick::Reply(AGAIN));
    ground.step();
    Ok((apart(ground.yaw(talker), east()), ground.off(host, talker)))
}

/// S2: the talker on the lock-orientation list
/// (`SetLockOrientationInDialog`) does not turn at the next line; the
/// host does.
fn lock_orientation() -> Found {
    let (turned, host) = relocked(Stage::default(), "probe_lock_turn")?;
    check(
        turned < 1.0 && host < 1.0,
        format!("the talker turned {turned:.1}°, the host {host:.1}° off it"),
    )
}

/// S2: with a head arc of 30° but on the head-follow lock list
/// (`SetLockHeadFollowInDialog`), the talker turns the full 90°.
fn lock_head_turns_fully() -> Found {
    let stage = Stage {
        arc: 30.0,
        ..Stage::default()
    };
    let (turned, host) = relocked(stage, "probe_lock_head")?;
    check(
        (turned - 90.0).abs() < 1.0 && host < 1.0,
        format!("the talker turned {turned:.1}°, the host {host:.1}° off it"),
    )
}

/// S2: the talker locked in one conversation (`SetLockOrientationInDialog`)
/// is on no list in the next, and its first line turns it.
fn locks_cleared() -> Found {
    let (mut ground, [talker, host, _]) = facing(Stage::default(), FACING_SOUTH)?;
    ground.run_script("probe_lock_turn", TALKER)?;
    let locked = |ground: &Ground| {
        (ground.last.talk.conversations.first()).map(|c| c.lock_turn.contains(&ObjectId(talker.0)))
    };
    let first = locked(&ground);
    ground.goodbye()?;
    ground.turn(talker, east());
    begin(&mut ground)?;
    let second = locked(&ground);
    let off = ground.off(talker, host);
    check(
        first == Some(true) && second == Some(false) && off < 1.0,
        format!("locked in the first {first:?}, the second {second:?}; {off:.1}° off the host"),
    )
}

/// S2: in an `AnimatedCut` DLG nobody turns at a line's start.
fn animated_cut_no_turn() -> Found {
    let stage = talk_with(|talk| talk.animated_cut = true);
    let (ground, [talker, host, _]) = facing(stage, FACING_SOUTH)?;
    let turned = [talker, host].map(|who| apart(ground.yaw(who), east()));
    check(
        turned.iter().all(|turned| *turned < 1.0),
        format!("the talker and the host turned {}", degrees(&turned)),
    )
}

/// S2: an entry naming the companion its `Listener`: it and the talker
/// turn to each other, and the host does not turn.
fn listener_tag() -> Found {
    let stage = talk_with(|talk| talk.entries[0].listener = "buddy".into());
    let (ground, [talker, host, buddy]) = facing(stage, FACING_SOUTH)?;
    let offs = [ground.off(talker, buddy), ground.off(buddy, talker)];
    let host_turned = apart(ground.yaw(host), east());
    let listener = ground.line().map(|(_, listener)| listener);
    check(
        offs.iter().all(|off| *off < 1.0)
            && host_turned < 1.0
            && listener == Some(ObjectId(buddy.0)),
        format!(
            "the talker and the companion off each other: {}; the host turned \
             {host_turned:.1}°, the listener {listener:?}",
            degrees(&offs)
        ),
    )
}

/// S2: an entry's `Listener` naming no one: the line is said to the host,
/// which turns to the talker.
fn listener_tag_missing() -> Found {
    let stage = talk_with(|talk| talk.entries[0].listener = "nobody".into());
    let (ground, [talker, host, _]) = facing(stage, FACING_SOUTH)?;
    let off = ground.off(host, talker);
    let listener = ground.line().map(|(_, listener)| listener);
    check(
        off < 1.0 && listener == Some(ObjectId(host.0)),
        format!("the host {off:.1}° off the talker, the listener {listener:?}"),
    )
}

/// S2: the talker and the companion speak in turn, naming no listener:
/// each listens to the other, and they face each other.
fn listener_previous_speaker() -> Found {
    let stage = Stage {
        talk: in_turn(&["", "buddy", ""]),
        ..Stage::default()
    };
    let (mut ground, [talker, _, buddy]) = facing(stage, FACING_EAST)?;
    let mut lines = Vec::new();
    for entry in [1, 2] {
        ground.until_entry(entry)?;
        let listener = ground.line().map(|(_, listener)| listener);
        lines.push((
            listener,
            ground.off(talker, buddy),
            ground.off(buddy, talker),
        ));
    }
    let id = |who: SimId| Some(ObjectId(who.0));
    let faced = lines.iter().all(|(_, a, b)| *a < 1.0 && *b < 1.0);
    check(
        lines[0].0 == id(talker) && lines[1].0 == id(buddy) && faced,
        format!("(listener, talker off, companion off) at entries 1 and 2: {lines:?}"),
    )
}

/// S2: a second line naming the talker its `Listener`, by a `Speaker`
/// naming the talker: it is said to the talker itself, as the data has it.
/// By a `Speaker` that names no one here it is never shown: both
/// executables pass such a line over (`Step::speaker_here`), where the
/// owner used to say it.
fn listener_self() -> Found {
    let line = |speaker: &str| -> Result<(Result<Option<ObjectId>, String>, SimId), String> {
        let mut talk = in_turn(&["", speaker]);
        talk.entries[1].listener = "talker".into();
        let stage = Stage {
            talk,
            ..Stage::default()
        };
        let (mut ground, [talker, ..]) = facing(stage, FACING_SOUTH)?;
        let shown = ground
            .until_entry(1)
            .map(|_| ground.line().map(|(_, listener)| listener));
        Ok((shown, talker))
    };
    let (missing, _) = line("nobody")?;
    let (named, talker) = line("talker")?;
    check(
        missing.is_err() && named == Ok(Some(ObjectId(talker.0))),
        format!("by a missing speaker {missing:?}, by the talker named to {named:?}"),
    )
}

/// TSL: a line whose `Speaker` stands farther from the owner than the
/// speakers' reach (`dialogue::speakers::REACH_M`) is passed over, unless
/// `SetForceAlwaysUpdate` flagged it (`State::always_update`): then it says
/// the line, as TSL's `GetNearestObjectByName` lets such an object past
/// its radius (512OND: Dhagon's call to the throne room, Kavar and Talia
/// 1400 m off, flagged by `a_force_update` the line before).
fn far_speaker_always_update() -> Found {
    let far = [-5.0 - 1.5 * rules::dialogue::speakers::REACH_M, 0.0];
    let shown = |flagged: bool| -> Result<bool, String> {
        let stage = Stage {
            talk: in_turn(&["", "buddy", ""]),
            ..Stage::default()
        };
        let (mut ground, [_, _, buddy]) = placed(stage, far)?;
        if flagged {
            let mut state = ground.world.script_state().clone();
            state.always_update.insert(buddy.0);
            ground.world.set_script_state(state);
        }
        begin(&mut ground)?;
        Ok(ground.until_entry(1).is_ok())
    };
    let (plain, flagged) = (shown(false)?, shown(true)?);
    check(
        !plain && flagged,
        format!("the far companion's line shown unflagged {plain}, flagged {flagged}"),
    )
}

/// S2: the talker speaks twice running, naming no listener: its second
/// line is said to the host, and turns the talker back to it.
fn same_speaker_to_player() -> Found {
    let stage = Stage {
        talk: in_turn(&["", ""]),
        ..Stage::default()
    };
    let (mut ground, [talker, host, _]) = facing(stage, FACING_SOUTH)?;
    ground.turn(talker, east());
    ground.until_entry(1)?;
    let listener = ground.line().map(|(_, listener)| listener);
    let off = ground.off(talker, host);
    check(
        listener == Some(ObjectId(host.0)) && off < 1.0,
        format!("the listener {listener:?}, the talker {off:.1}° off the host"),
    )
}

/// S2: the entry said to the companion (`Listener`), the host unturned;
/// once it has played, the replies offered are the host's line to the
/// talker: the talker listens, and the two turn to each other.
fn replies_offered_turn() -> Found {
    let stage = talk_with(|talk| talk.entries[0].listener = "buddy".into());
    let (mut ground, [talker, host, _]) = facing(stage, FACING_SOUTH)?;
    let before = apart(ground.yaw(host), east());
    ground.idle(300);
    ground.answer(Pick::Skip);
    ground.step();
    let offs = [ground.off(host, talker), ground.off(talker, host)];
    let listener = ground.line().map(|(_, listener)| listener);
    check(
        before < 1.0 && offs.iter().all(|off| *off < 1.0) && listener == Some(ObjectId(talker.0)),
        format!(
            "the host turned {before:.1}° by the entry; with the replies up, the host and the \
             talker off each other: {}, the listener {listener:?}",
            degrees(&offs)
        ),
    )
}

/// S2: "Goodbye." naming the companion its `Listener`, taken: the host
/// says it to the companion, and the two turn to each other.
fn reply_picked_turn() -> Found {
    let stage = talk_with(|talk| talk.replies[usize::from(GOODBYE)].listener = "buddy".into());
    let (mut ground, [talker, host, buddy]) = facing(stage, FACING_EAST)?;
    let before = ground.off(host, talker);
    ground.answer(Pick::Reply(GOODBYE));
    ground.step();
    let offs = [ground.off(host, buddy), ground.off(buddy, host)];
    check(
        before < 1.0 && offs.iter().all(|off| *off < 1.0),
        format!(
            "the host {before:.1}° off the talker at the entry; after the reply, the host and \
             the companion off each other: {}",
            degrees(&offs)
        ),
    )
}

/// How long the first line of a conversation staged as `stage` is meant to
/// last (ms), as it shows.
fn length(stage: Stage) -> Result<u64, String> {
    let mut ground = Ground::staged(1, stage)?;
    ground.cutscene()?;
    let c = (ground.last.talk.conversations.first()).ok_or("no conversation under way")?;
    Ok(c.until_ms - c.shown_ms)
}

/// The talker's first line saying `text` (none for ""), its `Delay` and
/// `WaitFlags` as given, a voice named "voice" of `voice_ms` where given.
fn line(text: &str, delay: Option<u32>, flags: u32, voice_ms: Option<u32>) -> Stage {
    let mut stage = talk_with(|talk| {
        let strings = if text.is_empty() {
            Vec::new()
        } else {
            vec![(0, text.as_bytes().to_vec())]
        };
        let node = &mut talk.entries[0];
        node.text = LocString {
            strref: u32::MAX,
            strings,
        };
        node.delay = delay;
        node.wait_flags = flags;
        node.sound = voice_ms.map_or_else(String::new, |_| "voice".into());
    });
    stage.voices = voice_ms
        .map(|ms| ("voice".to_string(), ms))
        .into_iter()
        .collect();
    stage
}

/// A line's length against `want` (ms).
fn lasts(stage: Stage, want: u64) -> Found {
    let ms = length(stage)?;
    check(ms == want, format!("the line lasts {ms} ms (want {want})"))
}

/// S5: an unvoiced line of 40 characters lasts 0.1125 s each.
fn line_by_text() -> Found {
    lasts(line(&"x".repeat(40), None, 0, None), 4500)
}

/// S5: a `Delay` above the text's length raises the line to it.
fn line_delay_raises() -> Found {
    lasts(line(&"x".repeat(40), Some(6), 0, None), 6000)
}

/// S5: a `Delay` under the text's length is the line's length, the text
/// not read (TSL's `CSWSDialog::SetDialogDelay` with a player in it).
fn line_delay_short() -> Found {
    lasts(line(&"x".repeat(40), Some(2), 0, None), 2000)
}

/// S5: a line with no text and no voice lasts the least, 10 ms.
fn silent_line_least() -> Found {
    lasts(line("", None, 0, None), 10)
}

/// S5: in an animated cut a line with no wait flags lasts nothing.
fn cut_line_zero() -> Found {
    let mut stage = line(&"x".repeat(40), None, 0, None);
    stage.talk.animated_cut = true;
    lasts(stage, 0)
}

/// S5: in an animated cut a voiced line with no wait flags waits for its
/// voice, as the games' loader gives it flag 2.
fn cut_voiced_line_waits() -> Found {
    let mut stage = line("Hi.", None, 0, Some(3000));
    stage.talk.animated_cut = true;
    lasts(stage, 3000)
}

/// S5: a voice of 5 s is a line's length, however short its text.
fn voice_is_base() -> Found {
    lasts(line("Hi.", None, 0, Some(5000)), 5000)
}

/// S5: a voice under 0.15 s is not the base: the text is.
fn short_voice_text() -> Found {
    lasts(line(&"x".repeat(40), None, 0, Some(100)), 4500)
}

/// S5: wait flag 2 holds a line with no text for its voice, 100 ms.
fn wait_voice_holds() -> Found {
    lasts(line("", None, 2, Some(100)), 100)
}

/// S5: wait flag 8 holds a line for its own fade, from 0.1 s in, as the K1
/// and TSL executables hold it: a fade in of 1 s delay and 2 s length
/// holds "Hi." (337 ms by its text) 2.9 s; a cover at once with 1.5 s
/// delay, its length not counted, 1.4 s; with no fade, or without the
/// flag, the text's length.
fn wait_fade_holds() -> Found {
    use fmt_odyssey::dlg::NodeFade;
    let faded = |flags: u32, kind: u8, (delay, fade_s): (f32, f32)| {
        let mut stage = line("Hi.", None, flags, None);
        stage.talk.entries[0].fade = NodeFade {
            kind,
            delay,
            length: fade_s,
            colour: [0.0; 3],
        };
        length(stage)
    };
    let got = [
        faded(8, NodeFade::IN, (1.0, 2.0))?,
        faded(8, NodeFade::COVER, (1.5, 2.0))?,
        faded(8, 0, (1.0, 2.0))?,
        faded(0, NodeFade::IN, (1.0, 2.0))?,
    ];
    check(
        got == [2900, 1400, 337, 337],
        format!("lines last {got:?} ms"),
    )
}

/// S5: wait flag 1 holds a line for its camera model's clip, its own timer
/// still counted, as the K1 and TSL executables end a line once its timer
/// and every flagged wait are done: "Hi." (337 ms by its text) with a 2 s
/// clip lasts 2 s; 40 characters (4.5 s) with the same clip 4.5 s; with
/// the flag and no clip of that name, the text's 337 ms.
fn wait_camera_clip_holds() -> Found {
    let filmed = |text: &str, animation: u16| {
        let mut stage = line(text, None, 1, None);
        stage.talk.camera_model = "probe_cam".into();
        stage.talk.entries[0].camera_animation = animation;
        stage.camera_clips = [(("probe_cam".to_string(), "cut001w".to_string()), 2000)].into();
        length(stage)
    };
    let long = "x".repeat(40);
    let got = [
        filmed("Hi.", 1200)?,
        filmed(&long, 1200)?,
        filmed("Hi.", 1201)?,
    ];
    check(got == [2000, 4500, 337], format!("lines last {got:?} ms"))
}

/// The conversation's first line after `ms` of the talker's pause script
/// `script` (`probe_pause`, …) running its line, a skip sent at 300 ms:
/// whether the line has played (its replies up) and whether it is paused.
fn after_pause(script: &str, ms: &[u64]) -> Result<Vec<(bool, bool)>, String> {
    let mut stage = line("Hello there.", None, 0, None);
    stage.talk.entries[0].script.script = script.into();
    let mut ground = Ground::staged(1, stage)?;
    ground.cutscene()?;
    ground.idle(300);
    ground.answer(Pick::Skip);
    let (mut seen, mut at) = (Vec::new(), 300);
    for &until in ms {
        ground.idle(until - at);
        at = until;
        let c = (ground.last.talk.conversations.first()).ok_or("the conversation ended")?;
        seen.push((c.until_ms == u64::MAX, c.paused));
    }
    Ok(seen)
}

/// TSL's `NodeUnskippable`: a skip sent 300 ms into a line of 4.5 s
/// ends it, unless the entry is marked, when the line plays on.
fn unskippable_line() -> Found {
    let played = |unskippable: bool| -> Result<bool, String> {
        let mut stage = line(&"x".repeat(40), None, 0, None);
        stage.talk.entries[0].unskippable = unskippable;
        let mut ground = Ground::staged(1, stage)?;
        ground.cutscene()?;
        ground.idle(300);
        ground.answer(Pick::Skip);
        ground.idle(100);
        let c = (ground.last.talk.conversations.first()).ok_or("the conversation ended")?;
        Ok(c.until_ms == u64::MAX)
    };
    let (plain, marked) = (played(false)?, played(true)?);
    check(
        plain && !marked,
        format!("played 100 ms after the skip: unmarked {plain}, unskippable {marked}"),
    )
}

/// TSL's alien speech (`Library::entry_voice`): four lines in turn, the
/// conversation's owner of race 1, whose emotion 1 is `avo_probe` (its
/// variants 1.5, 2 and 2.5 s long). The talker's first two lines (no race
/// of their own, no `Speaker`) play it as `probe_talk`'s walk draws the
/// variants, 3 then 1, and last as long; the third, of a race with nothing
/// for the emotion, and the fourth, naming its speaker, play none, the
/// walk moving on all the same. The lines have no `Delay`, which would
/// time them instead.
fn alien_speech() -> Found {
    let mut talk = in_turn(&["", "", "", "talker"]);
    talk.alien_race_owner = 1;
    for node in &mut talk.entries {
        node.emotion = 1;
        node.delay = None;
    }
    talk.entries[2].alien_race = 2;
    let stage = Stage {
        talk,
        aliens: vec![
            Vec::new(),
            vec![String::new(), "avo_probe".into()],
            vec![String::new(); 2],
        ],
        voices: (1..=3u32)
            .map(|n| (format!("avo_probe{n}"), 1000 + 500 * n))
            .collect(),
        ..Stage::default()
    };
    let mut ground = Ground::staged(1, stage)?;
    ground.cutscene()?;
    let mut seen = Vec::new();
    for entry in 0..4 {
        ground.until_entry(entry)?;
        let c = (ground.last.talk.conversations.first()).ok_or("the conversation ended")?;
        seen.push((c.voice_variant(entry), c.until_ms - c.shown_ms));
    }
    check(
        seen[..2] == [(Some(3), 2500), (Some(1), 1500)]
            && seen[2..].iter().all(|(v, _)| v.is_none()),
        format!("(variant, ms) by entry: {seen:?}"),
    )
}

/// S5: `ActionPauseConversation`, `ActionWait(3.0)` and
/// `ActionResumeConversation` run by the talker on its line: the line
/// (1350 ms, a skip sent at 300 ms) is held for the 3 s, and plays once
/// the resume has run.
fn pause_is_immediate() -> Found {
    let seen = after_pause("probe_pause", &[2000, 3400])?;
    check(
        seen == [(false, true), (true, false)],
        format!("(played, paused) at 2 s and 3.4 s: {seen:?}"),
    )
}

/// S5: the pause takes effect at once but the resume stays behind the
/// caller's actions: with the pause script waiting 2 s before it asks for
/// the pause, then 1 s more before the resume, a line of 1350 ms is held
/// the 3 s all the same.
fn pause_behind_actions() -> Found {
    let seen = after_pause("probe_pause_late", &[2500, 3400])?;
    check(
        seen == [(false, true), (true, false)],
        format!("(played, paused) at 2.5 s and 3.4 s: {seen:?}"),
    )
}

/// The pause keeps its place in the caller's queue: the talker's line
/// pauses, queues its resume and clears its own queue before either ran
/// (Citadel Station's Czerka men, `a_czerkaint`, clear theirs to walk in):
/// the pause went with the queue, as the executables' queued pause would
/// never have run, and the line (1350 ms, a skip sent at 300 ms) has played
/// by 2 s, not held for good.
fn pause_cleared_with_its_queue() -> Found {
    let seen = after_pause("probe_pause_cleared", &[2000])?;
    check(
        seen == [(true, false)],
        format!("(played, paused) at 2 s: {seen:?}"),
    )
}

/// What the talker did running an animation script: the animation its body
/// first played and when, when the walk queued behind the script first led
/// its queue, and when its body first played nothing after that animation
/// (ms after the step the script ran).
struct Played {
    acting: Option<(Acting, u64)>,
    walk: Option<u64>,
    ended: Option<u64>,
}

/// The talker, its model's bow `BOW_MS` and talk `TALK_MS` long, runs
/// `script` (`probe_bow_fast`, …), a walk to `WALK_TO` queued behind it,
/// over 4 s.
fn played(script: &str) -> Result<Played, String> {
    let stage = Stage {
        clips: [((0, BOW), BOW_MS), ((0, TALK), TALK_MS)].into(),
        ..Stage::default()
    };
    let mut ground = Ground::staged(1, stage)?;
    ground.run_script(script, TALKER)?;
    let talker = ground.object(TALKER)?;
    ground.queue(talker, move_to(WALK_TO, false, None));
    let mut played = Played {
        acting: None,
        walk: None,
        ended: None,
    };
    for step in 0..=steps(4000) {
        let ms = step as u64 * STEP_MS;
        let acting = ground.entity(talker).and_then(|e| e.fighter?.acting);
        if played.acting.is_none() {
            played.acting = acting.map(|acting| (acting, ms));
        } else if acting.is_none() && played.ended.is_none() {
            played.ended = Some(ms);
        }
        let front = ground.world.script_state().actions.get(&talker.0);
        if played.walk.is_none()
            && front
                .and_then(|queue| queue.first())
                .is_some_and(|front| matches!(front.action, Queue::Move { .. }))
        {
            played.walk = Some(ms);
        }
        ground.step();
    }
    Ok(played)
}

/// Whether `played` began `animation` for `ms` at once and walked on after
/// `walk_ms`, to a step.
fn plays(played: &Played, (animation, ms): (u16, u32), walk_ms: u64) -> bool {
    let near = |at: u64, want: u64| at.abs_diff(want) <= STEP_MS;
    played
        .acting
        .is_some_and(|(acting, at)| acting.animation == animation && acting.ms == ms && near(at, 0))
        && played.walk.is_some_and(|at| near(at, walk_ms))
}

/// S7: `ActionPlayAnimation`'s length in the queue. A fire-and-forget clip
/// plays once at its speed (a 1.4 s bow at speed 2 lasts 700 ms, and the
/// walk queued after it starts then), 1 s where the body's model has no
/// length for it; a looping one given 3 s lasts exactly that; a looping one
/// given −1 plays at once, outside the queue, which goes on (a 1 s wait
/// before it).
fn play_animation_lengths() -> Found {
    let cases = [
        ("probe_bow_fast", (BOW, BOW_MS / 2), 700),
        ("probe_victory", (VICTORY, 1000), 1000),
        ("probe_talk_3s", (TALK, 3000), 3000),
        ("probe_talk_on", (TALK, u32::MAX), 1000),
    ];
    let mut seen = Vec::new();
    for (script, play, walk_ms) in cases {
        let played = played(script)?;
        let fits = plays(&played, play, walk_ms);
        seen.push(format!(
            "{script}: {fits} (played {:?}, walk at {:?} ms)",
            played
                .acting
                .map(|(acting, at)| (acting.animation, acting.ms, at)),
            played.walk
        ));
    }
    check(
        seen.iter().all(|line| line.contains(": true")),
        seen.join("; "),
    )
}

/// S7: a looping animation played on at once (a duration of −1) loops until
/// something else plays: the walk queued behind the script's 1 s wait ends
/// it as it starts.
fn loop_ends_on_walk() -> Found {
    let played = played("probe_talk_on")?;
    let near = |at: Option<u64>| at.is_some_and(|at| at.abs_diff(1000) <= STEP_MS);
    check(
        played
            .acting
            .is_some_and(|(acting, _)| acting.ms == u32::MAX)
            && near(played.walk)
            && near(played.ended),
        format!(
            "looping on {:?}, the walk at {:?} ms, the loop ended at {:?} ms",
            played.acting.map(|(acting, at)| (acting.animation, at)),
            played.walk,
            played.ended
        ),
    )
}

/// S7: an animation queued for 30 s or more drops the same one queued after
/// it: two 40 s sleeps queue one.
fn long_animation_once() -> Found {
    let stage = Stage {
        clips: [((0, SLEEP), SLEEP_MS)].into(),
        ..Stage::default()
    };
    let mut ground = Ground::staged(1, stage)?;
    ground.run_script("probe_sleep_twice", TALKER)?;
    let queued = ground.queued(ground.object(TALKER)?);
    check(queued == 1, format!("{queued} sleeps queued"))
}

/// S5 bit 4: a line whose `WaitFlags` hold it for its `AnimList`, its owner
/// bowing for 2 s (`dialoganimations.2da` row 0), lasts the bow, past its
/// 1 s voice.
fn wait_for_gestures() -> Found {
    let mut stage = line("Hi.", None, 4, Some(1000));
    stage.talk.entries[0].anims = vec![("OWNER".into(), 10_000)];
    stage.gestures = vec![Some(BOW)];
    stage.clips = [((0, BOW), 2000)].into();
    lasts(stage, 2000)
}

impl Ground {
    /// `who` given `action` at the end of its queue, as a script queues it.
    fn queue(&mut self, who: SimId, action: Queue) {
        let mut state = self.world.script_state().clone();
        state.queue(ObjectId(who.0), action);
        self.world.set_script_state(state);
    }

    /// How many actions `who` has queued.
    fn queued(&self, who: SimId) -> usize {
        self.world
            .script_state()
            .actions
            .get(&who.0)
            .map_or(0, Vec::len)
    }

    /// This run's seats and answers on `world`, as `last` shows it, its
    /// steps `reason`'s.
    fn on(&self, world: SimWorld, last: Snapshot, reason: StepReason) -> Ground {
        Ground {
            world,
            tick: self.tick,
            seats: self.seats,
            replies: self.replies.clone(),
            last,
            reason,
        }
    }

    /// The run as a client predicts it from the last snapshot: no scripts,
    /// each step a prediction (as `coop_talk`'s probes predict).
    fn predicted(&self) -> Result<Ground, String> {
        let world = SimWorld::from_snapshot(&self.last, self.world.level().clone())
            .map_err(|error| error.to_string())?;
        Ok(self.on(world, self.last.clone(), StepReason::Predict))
    }

    /// The run as a save of it now loads (`Save`, through its bytes).
    fn reloaded(&self) -> Result<Ground, String> {
        let level = self.world.level();
        let save = crate::Save {
            hz: crate::TICK_HZ,
            level: level.id(),
            snapshot: self.last.clone(),
            script: self.world.script_state().clone(),
        };
        let save = crate::Save::decode(&save.encode()).map_err(|why| why.to_string())?;
        let world = save.world(level, self.seats, None)?;
        Ok(self.on(world, save.snapshot, StepReason::Authority))
    }

    /// `who`'s movement state.
    fn ps(&self, id: SimId) -> Option<crate::motion::MotionState> {
        self.entity(id).and_then(|entity| entity.player)
    }
}

/// An action as the scripts queue it.
type Queue = crate::script::actions::Action;

/// `ActionMoveToLocation` (or, with a timeout, `ActionForceMoveToLocation`)
/// to `to` on the ground's floor, as the routine queues it.
fn move_to(to: [f32; 2], run: bool, force_ms: Option<u32>) -> Queue {
    Queue::Move {
        to: Target::Point(floor_at(to), 0.0),
        run,
        range: crate::script::MOVE_TO_LOCATION_RANGE,
        force_ms,
    }
}

/// Where the walker starts its walks and where they end, 10 m on along
/// the ground's floor, clear of its ramp and wedge.
const WALK_FROM: [f32; 2] = [-2.0, -5.0];
/// Where the walking probes stand the buddy, the foe and the dummy.
const ASIDE: [[f32; 2]; 3] = [[-7.0, 1.0], [-7.0, 3.0], [-5.0, 4.5]];
const WALK_TO: [f32; 2] = [-2.0, 5.0];

/// KOTOR's walk and run of the walker (`creaturespeed.2da`'s `NORMAL`
/// row, m/s), and how near a speed must come to one.
const KOTOR_WALK: f32 = 1.7;
const KOTOR_RUN: f32 = 5.4;
const SPEED_SHARE: f32 = 0.02;

/// A walk as it went: after each step, where the mover stood (m), where it
/// faced (degrees), its velocity (m/s), whether KOTOR's walker carried it,
/// whether its move was still queued, and its walker's bumps.
#[derive(Default)]
struct Track {
    at: Vec<[f32; 3]>,
    yaw: Vec<f32>,
    velocity: Vec<[f32; 3]>,
    carried: Vec<bool>,
    queued: Vec<bool>,
    bumps: Vec<Option<u8>>,
    stalls: Vec<Option<u8>>,
}

impl Track {
    /// One step more of `who` on `ground`.
    fn record(&mut self, ground: &Ground, who: SimId) {
        let ps = ground.ps(who);
        self.at.push(ground.at(who));
        self.yaw.push(ground.yaw(who));
        self.velocity.push(
            ground
                .entity(who)
                .map_or([0.0; 3], |entity| entity.velocity),
        );
        self.carried
            .push(ps.is_some_and(|ps| ps.flags & crate::motion::motion_flags::CARRIED != 0));
        self.queued.push(ground.queued(who) > 0);
        let walker = ground
            .entity(who)
            .and_then(|entity| entity.fighter)
            .and_then(|fighter| fighter.mind.walker);
        self.bumps.push(walker.map(|walker| walker.bumps));
        self.stalls.push(walker.map(|walker| walker.stalls));
    }

    /// The step its move ended on (none was still queued after it).
    fn ended(&self) -> Option<usize> {
        self.queued.iter().position(|queued| !queued)
    }

    /// Its speed (m/s) between `a` and `b` metres along the way from
    /// `from` toward `to`.
    fn speed(&self, from: [f32; 2], to: [f32; 2], (a, b): (f32, f32)) -> Option<f32> {
        let along = |p: &[f32; 3]| along(from, to, *p);
        let first = self.at.iter().position(|p| along(p) >= a)?;
        let last = self.at.iter().position(|p| along(p) >= b)?;
        let (gone, steps) = (
            along(&self.at[last]) - along(&self.at[first]),
            last.checked_sub(first).filter(|steps| *steps > 0)?,
        );
        Some(gone / (steps as f32 * STEP_MS as f32 / 1000.0))
    }
}

/// How far (m) `p` stands along the way from `from` toward `to` (points of
/// the ground's floor).
fn along(from: [f32; 2], to: [f32; 2], p: [f32; 3]) -> f32 {
    let (from, to) = (floor_at(from), floor_at(to));
    ((p[0] - from[0]) * (to[0] - from[0]) + (p[1] - from[1]) * (to[1] - from[1]))
        / between(from, to)
}

/// `who` given `action` (queued now), stepped `ms`: its track.
fn walked(ground: &mut Ground, who: SimId, action: Queue, ms: u64) -> Track {
    ground.queue(who, action);
    let mut track = Track::default();
    for _ in 0..steps(ms) {
        ground.step();
        track.record(ground, who);
    }
    track
}

/// The ground with the walker standing at `at`, the host and the talker
/// aside, and (`held`) a party conversation holding the world.
fn walking_ground(at: [f32; 2], held: bool) -> Result<(Ground, SimId), String> {
    walking_on(Stage::default(), at, held)
}

/// `walking_ground` on `stage`.
fn walking_on(stage: Stage, at: [f32; 2], held: bool) -> Result<(Ground, SimId), String> {
    let mut ground = Ground::staged(
        1,
        Stage {
            walker: true,
            ..stage
        },
    )?;
    let host = ground.body(0)?;
    ground.put(host, HOST_AT);
    ground.place(TALKER, TALKER_AT)?;
    // The others out of the walk's lane: the loader stands the waiting
    // creatures apart (`install::settle`), one of them onto it.
    for (index, aside) in [BUDDY, FOE, DUMMY].into_iter().zip(ASIDE) {
        ground.place(index, aside)?;
    }
    let walker = ground.place(WALKER, at)?;
    ground.step();
    if held {
        ground.cutscene()?;
    }
    Ok((ground, walker))
}

/// Whether `speed` is `want` within `SPEED_SHARE`.
fn near_speed(speed: Option<f32>, want: f32) -> bool {
    speed.is_some_and(|speed| (speed - want).abs() <= want * SPEED_SHARE)
}

/// S3: a script's walk 10 m on, the world held, crosses at the walker's
/// `creaturespeed.2da` walk, 1.70 m/s; its run back at 5.40.
fn walks_at_kotor_speed() -> Found {
    let (mut ground, walker) = walking_ground(WALK_FROM, true)?;
    let walk = walked(&mut ground, walker, move_to(WALK_TO, false, None), 8000);
    let walking = walk.speed(WALK_FROM, WALK_TO, (2.0, 8.0));
    let run = walked(&mut ground, walker, move_to(WALK_FROM, true, None), 3000);
    let running = run.speed(WALK_TO, WALK_FROM, (2.0, 8.0));
    check(
        near_speed(walking, KOTOR_WALK) && near_speed(running, KOTOR_RUN),
        format!("walked at {walking:?} m/s, ran at {running:?} m/s"),
    )
}

/// S3: a body a script's walk carries plays its walk, not an animation
/// begun before it (a line's gesture as the scene sends it off): the
/// walker, talking for 3 s (`probe_talk_3s`), is sent 10 m on with the
/// world held; its talk ends as its walk first moves it, and nothing plays
/// on it while it walks.
fn walk_ends_animation() -> Found {
    let (mut ground, walker) = walking_ground(WALK_FROM, true)?;
    ground.run_script("probe_talk_3s", WALKER)?;
    let talking = |ground: &Ground| {
        ground
            .entity(walker)
            .and_then(|entity| entity.fighter?.acting)
            .filter(|acting| acting.playing(ground.last.time_ms))
            .is_some()
    };
    if !talking(&ground) {
        return Err("the talk did not begin".into());
    }
    let mut state = ground.world.script_state().clone();
    state.actions.remove(&walker.0);
    ground.world.set_script_state(state);
    ground.queue(walker, move_to(WALK_TO, false, None));
    let (mut moved, mut talked_moving) = (0, 0);
    let mut before = ground.at(walker);
    for _ in 0..steps(1500) {
        ground.step();
        let at = ground.at(walker);
        if between(before, at) > 0.001 {
            moved += 1;
            talked_moving += usize::from(talking(&ground));
        }
        before = at;
    }
    check(
        moved > 0 && talked_moving == 0,
        format!("{moved} steps moved, {talked_moving} of them with the talk playing"),
    )
}

/// S3: the walk's yaw is the heading of its step, at every step it moves.
fn faces_its_path() -> Found {
    const DIAGONAL: [f32; 2] = [-5.0, 4.0];
    let (mut ground, walker) = walking_ground([-1.5, -2.0], true)?;
    let track = walked(&mut ground, walker, move_to(DIAGONAL, false, None), 6000);
    let mut worst: f32 = 0.0;
    let mut moving = 0;
    for step in 1..track.at.len() {
        let (a, b) = (track.at[step - 1], track.at[step]);
        if between(a, b) < 0.001 {
            continue;
        }
        moving += 1;
        let heading = crate::math::atan2_deg(b[1] - a[1], b[0] - a[0]);
        worst = worst.max(apart(track.yaw[step], heading));
    }
    check(
        moving > 100 && worst < 0.5,
        format!("{moving} steps moving, the yaw at most {worst:.3}° off the step"),
    )
}

/// A navigation mesh baked for a walker's body over `floor`, triangles
/// (metres) on the ground at height 0.
fn flat_mesh(floor: &[[[f32; 2]; 3]]) -> collision::nav::NavMesh {
    let floor: Vec<[[f32; 3]; 3]> = floor
        .iter()
        .map(|face| face.map(|[x, y]| [x, y, 0.0]))
        .collect();
    collision::nav::NavMesh::build(&floor, crate::walk::CLEARANCE * UNIT)
}

/// The corners of the route over `nav` from `from` to `to`.
fn flat_route(
    nav: &collision::nav::NavMesh,
    from: [f32; 3],
    to: [f32; 3],
) -> Result<Vec<[f32; 3]>, String> {
    nav.route(from, to, false, &|_| true)
        .ok_or_else(|| format!("no route from {from:?} to {to:?}"))
}

/// S3: a walker standing within a body's width of the way out of its face
/// heads for the goal it sees, not back round a corner behind it. A room, a
/// door frame whose slanted edge meets the room's wall at an inside corner
/// (the floor round it 187°), the doorway, a corridor and the goal's room,
/// as by Malak's door in the Leviathan's hangar: the walker stands 0.17 m
/// short of the frame's edge, and that corner, stepped a body's width off
/// its walls, lies past it.
fn route_from_doorway_heads_on() -> Found {
    const FLOOR: [[[f32; 2]; 3]; 11] = [
        [[12.0, -4.0], [13.5, -7.0], [18.0, -7.0]],
        [[12.0, -4.0], [18.0, -7.0], [18.0, 4.0]],
        [[12.0, -4.0], [18.0, 4.0], [12.0, 4.0]],
        [[12.0, -4.0], [12.0, 4.0], [11.0, 2.5]],
        [[11.0, 2.5], [11.0, -2.5], [12.0, -4.0]],
        [[11.0, -2.5], [11.0, 2.5], [7.5, 2.5]],
        [[7.5, 2.5], [7.5, -2.5], [11.0, -2.5]],
        [[7.5, -2.5], [7.5, 2.5], [1.0, 2.5]],
        [[1.0, 2.5], [1.0, -2.5], [7.5, -2.5]],
        [[1.0, -2.5], [1.0, 2.5], [-1.0, 2.5]],
        [[-1.0, 2.5], [-1.0, -2.5], [1.0, -2.5]],
    ];
    const FROM: [f32; 3] = [11.55, 0.0, 0.0];
    const TO: [f32; 3] = [-0.5, 0.0, 0.0];
    let corners = flat_route(&flat_mesh(&FLOOR), FROM, TO)?;
    let bearing = |p: [f32; 3]| crate::math::atan2_deg(p[1] - FROM[1], p[0] - FROM[0]);
    let off = apart(bearing(corners[0]), bearing(TO));
    check(
        off < 10.0,
        format!(
            "the first corner {:?} {off:.1}° off the goal's bearing, corners {corners:?}",
            corners[0]
        ),
    )
}

/// S3: a walker beside an outside corner (a block's, one the way bends
/// round), clear of the walls by more than a body's width, still turns round
/// it stepped off the walls, though it stands past the line through the
/// doorway's stepped ends: a room east of a doorway whose south side is the
/// block's corner, the row of rooms west of it, and the goal's room north at
/// its end.
fn route_rounds_outside_corner() -> Found {
    const FLOOR: [[[f32; 2]; 3]; 14] = [
        [[17.5, 12.5], [20.0, 12.5], [17.5, 15.0]],
        [[20.0, 12.5], [20.0, 15.0], [17.5, 15.0]],
        [[17.5, 10.0], [20.0, 10.0], [20.0, 12.5]],
        [[17.5, 10.0], [20.0, 12.5], [17.5, 12.5]],
        [[15.0, 12.5], [17.5, 12.5], [17.5, 15.0]],
        [[15.0, 12.5], [17.5, 15.0], [15.0, 15.0]],
        [[12.5, 12.5], [15.0, 12.5], [15.0, 15.0]],
        [[12.5, 12.5], [15.0, 15.0], [12.5, 15.0]],
        [[10.0, 12.5], [12.5, 12.5], [12.5, 15.0]],
        [[10.0, 12.5], [12.5, 15.0], [10.0, 15.0]],
        [[7.5, 12.5], [10.0, 12.5], [10.0, 15.0]],
        [[7.5, 12.5], [10.0, 15.0], [7.5, 15.0]],
        [[7.5, 15.0], [10.0, 15.0], [10.0, 17.5]],
        [[7.5, 15.0], [10.0, 17.5], [7.5, 17.5]],
    ];
    // The block's corner, and where a body turns round it (`CLEARANCE`, a
    // box's reach across the corner's bisector).
    const CORNER: [f32; 2] = [17.5, 12.5];
    let turn = crate::walk::CLEARANCE * UNIT;
    const FROM: [f32; 3] = [18.15, 12.55, 0.0];
    const TO: [f32; 3] = [8.75, 16.25, 0.0];
    let corners = flat_route(&flat_mesh(&FLOOR), FROM, TO)?;
    let first = corners[0];
    let off = [first[0] - CORNER[0] - turn, first[1] - CORNER[1] - turn];
    check(
        off[0].hypot(off[1]) < 0.01,
        format!("the first corner {first:?}, corners {corners:?}"),
    )
}

/// The floor of squares `(x0, y0, x1, y1)` (metres), each two triangles.
fn squares(cells: &[[f32; 4]]) -> Vec<[[f32; 2]; 3]> {
    cells
        .iter()
        .flat_map(|&[x0, y0, x1, y1]| {
            [
                [[x0, y0], [x1, y0], [x1, y1]],
                [[x0, y0], [x1, y1], [x0, y1]],
            ]
        })
        .collect()
}

/// S3: a walker in a doorway between two blocks' corners, past the line
/// through the doorway's stepped ends, still turns round the far block's
/// corner stepped off its walls: both ends are outside corners, so neither
/// is dropped (dropped, the way cut the near block's corner at 0.45 m).
fn route_keeps_doorway_corners() -> Found {
    const CELLS: [[f32; 4]; 6] = [
        [2.5, 17.5, 5.0, 20.0],
        [5.0, 17.5, 7.5, 20.0],
        [7.5, 17.5, 10.0, 20.0],
        [2.5, 15.0, 5.0, 17.5],
        [5.0, 20.0, 7.5, 22.5],
        [7.5, 15.0, 10.0, 17.5],
    ];
    // The far block's corner, and where a body turns round it.
    const CORNER: [f32; 2] = [5.0, 20.0];
    let turn = crate::walk::CLEARANCE * UNIT;
    const FROM: [f32; 3] = [4.6, 17.95, 0.0];
    const TO: [f32; 3] = [8.75, 16.25, 0.0];
    let corners = flat_route(&flat_mesh(&squares(&CELLS)), FROM, TO)?;
    let first = corners[0];
    let off = [first[0] - CORNER[0] - turn, first[1] - CORNER[1] + turn];
    check(
        off[0].hypot(off[1]) < 0.01,
        format!("the first corner {first:?}, corners {corners:?}"),
    )
}

/// S3: a move to a place ends within 0.1 m of it, and stays there.
fn arrives_exactly() -> Found {
    let (mut ground, walker) = walking_ground(WALK_FROM, true)?;
    let track = walked(&mut ground, walker, move_to(WALK_TO, false, None), 8000);
    let ended = track.ended();
    let off = ended.map(|step| between(track.at[step], floor_at(WALK_TO)));
    let later = between(ground.at(walker), floor_at(WALK_TO));
    check(
        off.is_some_and(|off| off <= 0.1) && later <= 0.1,
        format!("ended at step {ended:?} {off:?} m off, then {later:.3} m off"),
    )
}

/// S3: `ActionMoveToObject` with its default range ends within 1.0 m of
/// the object, short of its body.
fn move_to_object_range() -> Found {
    let (mut ground, walker) = walking_ground(WALK_FROM, true)?;
    let dummy = ground.place(DUMMY, [-2.0, 3.0])?;
    let to = Queue::Move {
        to: Target::Object(ObjectId(dummy.0)),
        run: false,
        range: crate::script::MOVE_TO_LOCATION_RANGE,
        force_ms: None,
    };
    let track = walked(&mut ground, walker, to, 8000);
    let ended = track.ended();
    let off = ended.map(|step| between(track.at[step], ground.at(dummy)));
    let later = between(ground.at(walker), ground.at(dummy));
    check(
        off.is_some_and(|off| off <= 1.0) && (0.7..=1.0).contains(&later),
        format!("ended at step {ended:?} {off:?} m off, then {later:.3} m off"),
    )
}

/// S3: a body standing on the walk's line: the walker goes round it,
/// never into it, and arrives.
fn goes_round_a_body() -> Found {
    let (mut ground, walker) = walking_ground([-2.0, -4.0], true)?;
    let dummy = ground.place(DUMMY, [-2.0, 0.0])?;
    ground.step();
    let track = walked(&mut ground, walker, move_to([-2.0, 4.0], false, None), 9000);
    let nearest = track
        .at
        .iter()
        .map(|p| between(*p, ground.at(dummy)))
        .fold(f32::MAX, f32::min);
    let off = between(ground.at(walker), floor_at([-2.0, 4.0]));
    check(
        nearest >= 0.76 && off <= 0.1 && track.ended().is_some(),
        format!("{nearest:.2} m from the body at the nearest, {off:.3} m off the end"),
    )
}

/// Three bodies across the ground at y = 0 (the dummy, the companion and
/// the brawler), every side a walker meeting the middle one could go round
/// it by taken or off the mesh; each busy (a long wait queued), so none is
/// pushed aside.
fn wall(ground: &mut Ground) -> Result<(), String> {
    for (index, at) in [
        (DUMMY, [-2.0, 0.0]),
        (BUDDY, [-3.8, 0.0]),
        (FOE, [-1.1, 0.0]),
    ] {
        let body = ground.place(index, at)?;
        ground.queue(body, Queue::Wait { ms: 60_000 });
    }
    ground.step();
    Ok(())
}

/// S3: walled in by three bodies, the walk gives up: its move ends within
/// 2 s, short of the wall.
fn gives_up_blocked() -> Found {
    let (mut ground, walker) = walking_ground([-2.0, -3.0], true)?;
    wall(&mut ground)?;
    let (from, to) = ([-2.0, -3.0], [-2.0, 3.0]);
    let track = walked(&mut ground, walker, move_to(to, false, None), 2000);
    let ended = track.ended();
    let reached = track
        .at
        .iter()
        .map(|p| along(from, to, *p))
        .fold(f32::MIN, f32::max);
    let bumps = track.bumps.iter().flatten().max().copied();
    check(
        ended.is_some() && reached < 3.0 && bumps.is_some_and(|bumps| bumps > 0),
        format!("ended at step {ended:?}, {reached:.2} m on of the wall's 3, bumps {bumps:?}"),
    )
}

/// S3: KOTOR's walker on the open floor (`walk::kotor_step`) bumping into
/// a new body in its way each step goes round the first five and gives the
/// walk up on the sixth; one bumping into the same body twice where it
/// stands gives up on the second.
fn bump_rules() -> Found {
    let ground = Ground::new(1)?;
    let level = ground.world.level();
    let stand = |at: [f32; 2]| {
        let [x, y, z] = floor_at(at).map(|v| v / UNIT);
        crate::motion::MotionState::standing([x, y, z + 0.125 - crate::motion::SOLE_Z], 0.0)
    };
    let me = stand([-4.0, -3.0]);
    let goal = stand([-4.0, 3.0]).origin;
    // A body a little way ahead of `me` toward its walker's corner.
    let ahead = |route: &crate::walk::Walker| {
        let to = route.waypoint.unwrap_or(route.goal);
        let way = crate::math::sub(to, me.origin);
        let far = way[0].hypot(way[1]);
        [
            me.origin[0] + way[0] / far * 20.0,
            me.origin[1] + way[1] / far * 20.0,
            me.origin[2],
        ]
    };
    let bump = |route: &mut crate::walk::Walker, who: u32| {
        let body = crate::walk::Met {
            who: SimId(who),
            at: ahead(route),
            push: None,
            going: None,
        };
        crate::walk::kotor_step(
            level,
            &me,
            route,
            KOTOR_WALK,
            STEP_MS as u32,
            false,
            0.35,
            &[body],
            &|_| crate::snapshot::ObjectState::default(),
        )
        .and_then(|stepped| stepped.bumped)
    };
    let mut new = crate::walk::Walker::new(goal);
    new.waypoint = Some(goal);
    let mut route = new;
    let mut stuck = Vec::new();
    for who in 100..106 {
        let bumped = bump(&mut route, who);
        stuck.push((bumped.map(|id| id.0), route.bumps, route.stuck()));
    }
    let mut twice = new;
    let first = bump(&mut twice, 100);
    // Done going round it, and meeting it again where it stands.
    twice.rounding = false;
    let second = bump(&mut twice, 100);
    let fifth = stuck[4];
    check(
        stuck.iter().all(|(bumped, ..)| bumped.is_some())
            && !fifth.2
            && stuck[5].2
            && first.is_some()
            && second.is_some()
            && twice.stuck(),
        format!(
            "(bumped, bumps, given up) a new body each step: {stuck:?}; the same twice: \
             given up {}",
            twice.stuck()
        ),
    )
}

/// KOTOR's walker running into a body it may push (idle, not hostile)
/// puts it aside out of its way, on a free spot, and walks on; one it may
/// not push it bumps into and stands.
fn pushes_idle_aside() -> Found {
    let ground = Ground::new(1)?;
    let level = ground.world.level();
    let stand = |at: [f32; 2]| {
        let [x, y, z] = floor_at(at).map(|v| v / UNIT);
        crate::motion::MotionState::standing([x, y, z + 0.125 - crate::motion::SOLE_Z], 0.0)
    };
    let me = stand([-4.0, -3.0]);
    let goal = stand([-4.0, 3.0]).origin;
    let body = [me.origin[0], me.origin[1] + 20.0, me.origin[2]];
    let step = |push: Option<f32>| {
        let mut route = crate::walk::Walker::new(goal);
        route.waypoint = Some(goal);
        let met = crate::walk::Met {
            who: SimId(100),
            at: body,
            push,
            going: None,
        };
        let stepped = crate::walk::kotor_step(
            level,
            &me,
            &mut route,
            KOTOR_WALK,
            STEP_MS as u32,
            false,
            0.35,
            &[met],
            &|_| crate::snapshot::ObjectState::default(),
        );
        (stepped, route.bumps)
    };
    let (pushed, pushed_bumps) = step(Some(0.35));
    let (bumped, bumped_bumps) = step(None);
    let spot = pushed.and_then(|stepped| stepped.pushed);
    // Out of the way: beside the line from `me` to the goal by the two
    // personal spaces and a margin (0.8 m), at least the boxes' widths.
    let aside = spot.map(|(_, at)| {
        let way = [goal[0] - me.origin[0], goal[1] - me.origin[1]];
        let rel = [at[0] - me.origin[0], at[1] - me.origin[1]];
        (rel[0] * way[1] - rel[1] * way[0]).abs() / way[0].hypot(way[1]) * UNIT
    });
    check(
        pushed.is_some_and(|stepped| stepped.bumped.is_none() && stepped.velocity != [0.0; 3])
            && aside.is_some_and(|aside| (0.79..1.2).contains(&aside))
            && pushed_bumps == 0
            && bumped.is_some_and(|stepped| stepped.bumped == Some(SimId(100)))
            && bumped_bumps == 1,
        format!(
            "pushed to {spot:?} ({aside:?} m aside), bumps {pushed_bumps}; \
             not pushable: bumped {:?}, bumps {bumped_bumps}",
            bumped.and_then(|stepped| stepped.bumped)
        ),
    )
}

/// A walker meeting the idle dummy on its line pushes it aside; made
/// immobile (`Body::immobile`, `moverate` NOMOVE: a turret) the dummy
/// stands where it is, and the walker goes round it. Telos's hidden turret
/// in Slusk's office was pushed 0.9 m into the way out and held the player
/// against Slusk.
fn immobile_not_pushed() -> Found {
    let moved = |immobile: bool| -> Result<f32, String> {
        let (mut ground, walker) = walking_ground([-2.0, -4.0], true)?;
        let dummy = ground.place(DUMMY, [-2.0, 0.0])?;
        if immobile {
            let mut objects = ground
                .world
                .world
                .query::<(&Id, &mut super::super::Object)>();
            for (id, mut object) in objects.iter_mut(&mut ground.world.world) {
                if id.0 != dummy {
                    continue;
                }
                if let Some(facts) = object.facts.as_mut() {
                    let mut fixed = (**facts).clone();
                    fixed.body = Some(Body {
                        movement: Body::NOMOVE,
                        walk_mps: 0.0,
                        run_mps: 0.0,
                        ..fixed.body.unwrap_or(body(0.0))
                    });
                    *facts = Arc::new(fixed);
                }
            }
        }
        ground.step();
        let start = ground.at(dummy);
        walked(&mut ground, walker, move_to([-2.0, 4.0], false, None), 9000);
        Ok(between(start, ground.at(dummy)))
    };
    let (free, fixed) = (moved(false)?, moved(true)?);
    check(
        free > 0.3 && fixed < 0.01,
        format!("the idle dummy moved {free:.2} m, made immobile {fixed:.3} m"),
    )
}

/// How deep `me` stands inside `other` (units): how far their rounds
/// overlap, none once they are clear.
fn depth(me: &crate::motion::MotionState, other: &collision::Body) -> f32 {
    let centre = |axis: usize| (other.min[axis] + other.max[axis]) * 0.5 / UNIT;
    let apart = crate::math::flat_length([centre(0) - me.origin[0], centre(1) - me.origin[1], 0.0]);
    (crate::motion::HALF_WIDTH + (other.max[0] - other.min[0]) * 0.5 / UNIT - apart).max(0.0)
}

/// 60 steps of a seat's player standing as `standing` among `others`,
/// pressing `command`, as `world::movement` moves one (`clear_of`,
/// `move_body`, `Inside::hold_off`): where it ends, whether it stands on
/// the ground, and how deep inside any of the others it ever stood.
fn walk_among(
    level: &crate::world::level::Level,
    standing: crate::motion::MotionState,
    others: &[collision::Body],
    command: &Command,
) -> ([f32; 3], bool, f32) {
    let deepest_now = |me: &crate::motion::MotionState| {
        others
            .iter()
            .map(|other| depth(me, other))
            .fold(0.0, f32::max)
    };
    let mut me = standing;
    let mut deepest = deepest_now(&me);
    for tick in 0..60 {
        let mine = crate::world::met_as(&crate::world::body(SimId(1), &me), true);
        let inside = crate::world::clear_of(&mine, others);
        let before = me.origin;
        level.with(None, |world| {
            let world = collision::Bodies::new(world, &inside.bodies);
            crate::motion::move_body(
                &mut me,
                command,
                STEP_MS as u32,
                tick * STEP_MS,
                &world,
                level.anims(),
                None,
            );
        });
        let maxs_z = me.maxs_z;
        inside.hold_off(before, &mut me, |from, to| {
            level.with(None, |world| {
                let world = collision::Bodies::new(world, &inside.bodies);
                crate::world::movement::standing_clear(&world, from, to, maxs_z)
            })
        });
        deepest = f32::max(deepest, deepest_now(&me));
    }
    (me.origin, me.ground.is_some(), deepest)
}

/// A body met round across (`collision::Bodies`, `brush::clip_round`)
/// slides off round another, so three standing close about a player leave
/// it the way out between two of them that a body fits through, as KOTOR's
/// creatures meet by their personal space: met as square boxes, the player
/// coming out of Onderon's palace gas room stood in the corner of two royal
/// soldiers' boxes, every way out taken back (their corners 0.94 m and 0.90
/// m from its centre, the way between them 1.83 m across). Walking the way
/// out it gets 1 m out, never deeper into any of them than it stood.
fn body_pocket_walked_out() -> Found {
    let ground = Ground::new(1)?;
    let level = ground.world.level();
    let [x, y, z] = floor_at([-2.0, -3.0]).map(|v| v / UNIT);
    let standing =
        crate::motion::MotionState::standing([x, y, z + 0.125 - crate::motion::SOLE_Z], 0.0);
    // The soldiers about the Exile (metres from it), each as KOTOR's
    // humanoids keep their personal space (0.35 m, `world::blocking`).
    let space = 0.35 / UNIT;
    let others: Vec<collision::Body> = [[-0.74f32, -0.58], [0.57, 0.70], [0.17, -1.18]]
        .into_iter()
        .enumerate()
        .map(|(n, [dx, dy])| {
            let at = [x + dx / UNIT, y + dy / UNIT, standing.origin[2]];
            let mut body = crate::world::body(
                SimId(100 + n as u32),
                &crate::motion::MotionState::standing(at, 0.0),
            );
            body.min[0] = (at[0] - space) * UNIT;
            body.min[1] = (at[1] - space) * UNIT;
            body.max[0] = (at[0] + space) * UNIT;
            body.max[1] = (at[1] + space) * UNIT;
            body
        })
        .collect();
    let started = others
        .iter()
        .map(|other| depth(&standing, other))
        .fold(0.0, f32::max);
    // Toward the way out, between the first two.
    let out = [-0.74f32, 1.12];
    let yaw = crate::math::atan2_deg(out[1], out[0]);
    let command = Command {
        forward: 127,
        angles: [0, (yaw / 360.0 * 65536.0).round() as i32 as i16, 0],
        ..Command::default()
    };
    let (at, grounded, deepest) = walk_among(level, standing, &others, &command);
    let gone = crate::math::flat_distance(at, standing.origin) * UNIT;
    check(
        grounded && gone >= 1.0 && deepest <= started + 1e-3,
        format!(
            "from {:?} among {} bodies {started:.2} units into the nearest: walked {gone:.2} m to {at:?}, at most {deepest:.2} units into one",
            standing.origin,
            others.len()
        ),
    )
}

/// A body standing inside another (the player pressed against a creature
/// whose personal space is narrower than the box; one put inside another by
/// a script's jump) is not held by it (`world::clear_of`): standing still,
/// it stays where it stands on its floor, not nudged; walking away, it
/// walks out, though a step leaves it inside; walking at it, it gets no
/// nearer (`Inside::hold_off`); walking at it slantwise, it walks along it
/// and out past its side (a player stood into Freyyr against a wall in Chuundar's throne room,
/// every step its walk tried taken back whole, never got out). A seat's
/// player meets a body it only touches as one it stands inside
/// (`world::met_as`), a creature does not; touching it, the player walks
/// at it slantwise along it and out past its side too. Met off its middle
/// (the two rounds 3 units into each other, 20 degrees off the way),
/// walking at it slantwise goes round it, past it and no deeper: weighed by
/// the boxes' middles, every step was taken back, and the player stood held
/// at a mechanic's box by Anchorhead's docking bay door for good; weighed
/// as boxes, a body between two others' corners stood in their pocket.
fn body_overlap_not_buried() -> Found {
    let ground = Ground::new(1)?;
    let level = ground.world.level();
    let [x, y, z] = floor_at([-2.0, -3.0]).map(|v| v / UNIT);
    let standing =
        crate::motion::MotionState::standing([x, y, z + 0.125 - crate::motion::SOLE_Z], 0.0);
    // The body standing `inside` units into one standing as `standing`
    // across x, and `across` units along y from it.
    let off = |inside: f32, across: f32| {
        let other = crate::motion::MotionState::standing(
            [
                x + 2.0 * crate::motion::HALF_WIDTH - inside,
                y + across,
                standing.origin[2],
            ],
            0.0,
        );
        crate::world::body(SimId(100), &other)
    };
    let other = |inside: f32| off(inside, 0.0);
    // Its steps beside another body (as a seat's player), or alone,
    // pressing `forward` facing away from it (`-127`: toward it) and
    // `right`: where it ends, whether it stands on the ground, and how far
    // deep inside the other it ever stood.
    let walked = |other: Option<collision::Body>, forward: i8, right: i8| {
        let others: Vec<collision::Body> = other.into_iter().collect();
        let command = Command {
            forward,
            right,
            angles: [0, i16::MIN, 0],
            ..Command::default()
        };
        walk_among(level, standing, &others, &command)
    };
    let stood = |inside: Option<f32>, forward: i8, right: i8| {
        let (at, grounded, _) = walked(inside.map(other), forward, right);
        (at, grounded)
    };
    let (alone, inside) = (stood(None, 0, 0), stood(Some(1.0), 0, 0));
    let still = |(at, grounded): ([f32; 3], bool)| {
        grounded && at[0] == standing.origin[0] && at[1] == standing.origin[1]
    };
    let (out, _) = stood(Some(8.0), 127, 0);
    let (into, _) = stood(Some(8.0), -127, 0);
    let (slant, _) = stood(Some(8.0), -127, 127);
    // A gap of the trace's own (a move stopped by it leaves that much).
    let gap = -collision::CONTACT_GAP / UNIT;
    let mine = crate::world::body(SimId(1), &standing);
    let passed = |seated: bool| {
        crate::world::clear_of(&crate::world::met_as(&mine, seated), &[other(gap)])
            .bodies
            .is_empty()
    };
    let (seat_passes, creature_passes) = (passed(true), passed(false));
    let (touch_slant, _) = stood(Some(gap), -127, 127);
    // Off its middle: the rounds 3 units into each other, 20 degrees off x.
    let (sin, cos) = crate::math::sin_cos_deg(20.0);
    let apart = 2.0 * crate::motion::HALF_WIDTH - 3.0;
    let corner = off(2.0 * crate::motion::HALF_WIDTH - apart * cos, apart * sin);
    let started = depth(&standing, &corner);
    let (corner_at, _, corner_deepest) = walked(Some(corner), -127, 127);
    check(
        still(alone)
            && still(inside)
            && standing.origin[0] - out[0] > 20.0
            && into[0] <= standing.origin[0]
            && (slant[1] - standing.origin[1]).abs() >= 2.0 * crate::motion::HALF_WIDTH
            && seat_passes
            && !creature_passes
            && (touch_slant[1] - standing.origin[1]).abs() >= 2.0 * crate::motion::HALF_WIDTH
            && corner_deepest <= started
            && (corner_at[1] - standing.origin[1]).abs() >= 2.0 * crate::motion::HALF_WIDTH,
        format!(
            "from {:?}: alone {alone:?}, a unit inside another {inside:?}; 8 units inside \
             walking away: at {out:?}, walking at it: at {into:?}, slantwise: at {slant:?}; \
             touching: passed over for a seat {seat_passes}, for a creature \
             {creature_passes}, slantwise at {touch_slant:?}; off its middle {started} \
             units in, slantwise at {corner_at:?}, at most {corner_deepest} in",
            standing.origin
        ),
    )
}

/// A creature's walk (`Walker::command`) through `walk::Walled` keeps off
/// the faces `walk::MadeWalls` finds under a placeable play made across its
/// way: its corners go round the placeable's box, where its doors alone
/// take it straight through; and so does the player's click-to-walk
/// (`target::Walk`), as a view plans it from a snapshot.
fn walker_keeps_off_made_placeable() -> Found {
    let cells: Vec<[f32; 4]> = (0..8)
        .flat_map(|y| (0..8).map(move |x| [x as f32, y as f32, x as f32 + 1.0, y as f32 + 1.0]))
        .collect();
    let level = crate::Level::arena().with_nav(flat_mesh(&squares(&cells)));
    let at = [4.0, 4.0, 0.0];
    let speeder = ObjectData {
        kind: ObjectKind::Placeable,
        geometry: vec![[-1.0, -0.5, 0.0], [1.0, 0.5, 1.0]],
        ..ObjectData::unknown("", at, 0.0)
    };
    let mut walls = crate::walk::MadeWalls::default();
    let placed = level.placed().len() as u32;
    let speeder = std::sync::Arc::new(speeder);
    let walled = walls
        .faces(&level, &[(placed, at)], &|_| Some(speeder.clone()))
        .clone();
    let units = |[x, y]: [f32; 2]| [x / UNIT, y / UNIT, -crate::motion::SOLE_Z];
    let (from, to) = (units([4.0, 1.0]), units([4.0, 7.0]));
    let open = |_: u32| crate::snapshot::ObjectState::default();
    let ways = crate::walk::Walled {
        doors: &open,
        walled: &walled,
    };
    // The legs of the walk's route from `from`, and whether one passes over
    // the placeable's box.
    let crosses = |corners: &[[f32; 3]]| {
        let mut last = from;
        corners.iter().any(|corner| {
            let hit = (0..=32).any(|k| {
                let t = k as f32 / 32.0;
                let x = (last[0] + (corner[0] - last[0]) * t) * UNIT;
                let y = (last[1] + (corner[1] - last[1]) * t) * UNIT;
                (x - at[0]).abs() < 1.0 && (y - at[1]).abs() < 0.5
            });
            last = *corner;
            hit
        })
    };
    let plain = crate::walk::route(&level, from, to, &open);
    let round = crate::walk::route_ways(&level, from, to, &ways);
    let mut walker = crate::walk::Walker::new(to);
    let me = crate::motion::MotionState::standing(from, 90.0);
    walker.command(&level, &me, 0, &ways);
    let first = walker.waypoint;
    let off_line = first.is_some_and(|corner| (corner[0] * UNIT - at[0]).abs() > 0.5);
    // The player's click-to-walk (`target::Walk`) to a target past it,
    // planned through the same ways and through the doors alone: it heads
    // off the straight line only when keeping off the placeable.
    let click = |ways: &dyn crate::walk::Ways| {
        let target = [4.0, 7.0, 0.0];
        let mut walk = crate::target::Walk::new(
            &level,
            ways,
            &me,
            0,
            ObjectKind::Placeable,
            target,
            crate::target::Choice::Use,
            0,
        );
        match walk.stride(&level, ways, &me, ObjectKind::Placeable, target, 0) {
            crate::target::Stride::Steer(command) => Some(command.angles[1]),
            _ => None,
        }
    };
    let (click_round, click_plain) = (click(&ways), click(&open));
    check(
        walled.contains(&true)
            && plain.as_deref().is_some_and(crosses)
            && round.as_deref().is_some_and(|corners| !crosses(corners))
            && off_line
            && click_round.is_some()
            && click_plain.is_some()
            && click_round != click_plain,
        format!(
            "faces walled {}, plain route over it {:?}, round it {:?}; the walker's first \
             corner {first:?}; the click-to-walk's heading {click_round:?} (doors alone \
             {click_plain:?})",
            walled.iter().filter(|face| **face).count(),
            plain.as_deref().map(crosses),
            round.as_deref().map(crosses),
        ),
    )
}

/// Out of a scene, a creature's script walk at KOTOR's pace running into an
/// idle body in its way: a client predicting from the authority's snapshot
/// (which runs no scripts: the pace and who may be pushed come from the
/// minds' flags, `Mind::moving` and `Mind::queued`) walks it as the
/// authority does and pushes the body aside on the same step. (After the
/// push the authority's mind steers the walk on, which a client, running
/// no minds, takes from the next snapshot.)
fn free_walk_predicted() -> Found {
    let (mut ground, walker) = walking_ground(WALK_FROM, false)?;
    let dummy = ground.place(DUMMY, [-2.0, 0.0])?;
    ground.step();
    ground.queue(walker, move_to(WALK_TO, false, None));
    for _ in 0..steps(200) {
        ground.step();
    }
    let (from_walker, from_dummy) = (ground.at(walker), ground.at(dummy));
    let mut client = ground.predicted()?;
    let mut worst: f32 = 0.0;
    // The step each saw the body put aside.
    let (mut pushed, mut predicted) = (None, None);
    for step in 0..steps(4000) {
        ground.step();
        client.step();
        if pushed.is_none() {
            worst = worst.max(between(ground.at(walker), client.at(walker)));
        }
        if pushed.is_none() && between(ground.at(dummy), from_dummy) > 0.1 {
            pushed = Some(step);
        }
        if predicted.is_none() && between(client.at(dummy), from_dummy) > 0.1 {
            predicted = Some(step);
        }
    }
    let walked = between(ground.at(walker), from_walker);
    check(
        walked > 3.0 && worst < 0.001 && pushed.is_some() && pushed == predicted,
        format!(
            "the walker {walked:.2} m on, the client at most {worst:.4} m off it until the \
             push; the body pushed aside at step {pushed:?}, predicted at {predicted:?}"
        ),
    )
}

/// A route planned off the faces under a placeable play made
/// (`walk::faces_under_made`: the navigation mesh is baked before) keeps
/// its corners' legs out of the placeable's cell, where the plain route
/// runs through it.
fn routes_round_made_placeable() -> Found {
    // A floor of metre squares, 8 m by 8 m, the placeable 2 m by 1 m across
    // the middle of the way from its south edge to its north.
    let cells: Vec<[f32; 4]> = (0..8)
        .flat_map(|y| (0..8).map(move |x| [x as f32, y as f32, x as f32 + 1.0, y as f32 + 1.0]))
        .collect();
    let nav = flat_mesh(&squares(&cells));
    let at = [4.0, 4.0, 0.0];
    let speeder = ObjectData {
        kind: ObjectKind::Placeable,
        geometry: vec![[-1.0, -0.5, 0.0], [1.0, 0.5, 1.0]],
        ..ObjectData::unknown("", at, 0.0)
    };
    let under = crate::walk::made_faces(&nav, &speeder, at);
    const FROM: [f32; 3] = [4.0, 1.0, 0.0];
    const TO: [f32; 3] = [4.0, 7.0, 0.0];
    let plain = flat_route(&nav, FROM, TO)?;
    let round = nav
        .route_round(FROM, TO, false, &|_| true, &|face| {
            under.get(face as usize).copied().unwrap_or(false)
        })
        .ok_or("no way round")?;
    // Whether a leg of `corners` from `FROM` passes over the placeable.
    let crosses = |corners: &[[f32; 3]]| {
        let mut last = FROM;
        corners.iter().any(|corner| {
            let hit = (0..=32).any(|k| {
                let t = k as f32 / 32.0;
                let x = last[0] + (corner[0] - last[0]) * t;
                let y = last[1] + (corner[1] - last[1]) * t;
                (x - at[0]).abs() < 1.0 && (y - at[1]).abs() < 0.5
            });
            last = *corner;
            hit
        })
    };
    check(
        under.contains(&true) && crosses(&plain) && !crosses(&round),
        format!(
            "faces under it {}, the plain route {plain:?}, round it {round:?}",
            under.iter().filter(|face| **face).count()
        ),
    )
}

/// S3: the walker's rates: a player's the `PLAYER` row; a creature's its
/// own, the `DEFAULT` row where its own reads as nothing, none where `NOMOVE`
/// (and then no speed); KOTOR's speed with the multiplier clamped to
/// 0.125–1.5, a walk under 0.1 m/s none, a run at least 1 m/s.
fn walker_rates() -> Found {
    use crate::walk::{kotor_speed, rates};
    let table = crate::script::MoveRates {
        player: Some((3.2, 5.4)),
        default: Some((1.7, 5.4)),
    };
    let own = body(0.0);
    let unread = Body {
        walk_mps: 0.0,
        run_mps: 0.0,
        ..own
    };
    let immobile = Body {
        movement: 1,
        ..unread
    };
    let found = [
        rates(None, true, table),
        rates(Some(&own), false, table),
        rates(Some(&unread), false, table),
        rates(None, false, table),
        rates(Some(&immobile), false, table),
        rates(Some(&own), false, crate::script::MoveRates::default()),
        rates(None, true, crate::script::MoveRates::default()),
    ];
    let speeds = [
        kotor_speed((1.7, 5.4), 2.0, false),
        kotor_speed((1.7, 5.4), 0.05, false),
        kotor_speed((0.5, 5.4), 0.125, false),
        kotor_speed((1.7, 5.4), 0.125, true),
        kotor_speed((0.0, 0.0), 1.0, true),
    ];
    let want_rates = [
        Some((3.2, 5.4)),
        Some((1.7, 5.4)),
        Some((1.7, 5.4)),
        Some((1.7, 5.4)),
        Some((0.0, 0.0)),
        Some((1.7, 5.4)),
        None,
    ];
    let want_speeds = [1.7 * 1.5, 1.7 * 0.125, 0.0, 1.0, 0.0];
    check(
        found == want_rates && speeds == want_speeds,
        format!("rates {found:?}; speeds {speeds:?}"),
    )
}

/// S3: `ActionForceMoveToLocation` with a 2 s timeout to a place walled
/// off: short of it at 1.9 s, on it (it is free) once the 2 s are up.
fn forced_jumps_late() -> Found {
    const MARK: [f32; 2] = [-2.0, 3.0];
    let (mut ground, walker) = walking_ground([-2.0, -3.0], true)?;
    wall(&mut ground)?;
    let track = walked(&mut ground, walker, move_to(MARK, false, Some(2000)), 2300);
    let at = |ms: u64| between(track.at[steps(ms)], floor_at(MARK));
    let (early, late) = (at(1900), at(2200));
    check(
        early > 2.0 && late <= 0.1,
        format!("{early:.2} m off at 1.9 s, {late:.3} m off at 2.2 s"),
    )
}

/// S3: `ActionJumpToLocation` onto a mark another body stands on lands on
/// a free spot within 20 m, never inside that body, the world held.
fn jump_avoids_bodies() -> Found {
    jump_beside(true, 0.0, 0.65..=20.0)
}

/// The same with no conversation: one jump rule, held or not.
fn free_jump_avoids_bodies() -> Found {
    jump_beside(false, 0.0, 0.65..=20.0)
}

/// A body 0.8 m from the mark (K1's own jump marks stand 0.81 m apart)
/// leaves it free: two humanoids' personal spaces are 0.7 m. One 0.5 m
/// off does not.
fn jump_beside_close_body() -> Found {
    let close = jump_beside(false, 0.8, 0.0..=0.05)?;
    let near = jump_beside(false, 0.5, 0.15..=20.0)?;
    Ok(format!("{close}; {near}"))
}

/// A jump to a mark with a body `body_off` m from it, the world `held` or
/// not: where the jumper lands, `off` metres from the mark, and at least
/// two personal spaces from the body.
fn jump_beside(held: bool, body_off: f32, off: std::ops::RangeInclusive<f32>) -> Found {
    const MARK: [f32; 2] = [-3.0, 2.0];
    let (mut ground, walker) = walking_ground([-2.0, -3.0], held)?;
    let dummy = ground.place(DUMMY, [MARK[0] + body_off, MARK[1]])?;
    ground.step();
    let jump = Queue::Jump {
        to: Target::Point(floor_at(MARK), 0.0),
    };
    walked(&mut ground, walker, jump, 200);
    let apart = between(ground.at(walker), ground.at(dummy));
    let landed = between(ground.at(walker), floor_at(MARK));
    check(
        off.contains(&landed) && apart >= 0.65,
        format!("{landed:.2} m from the mark, {apart:.2} m from the body {body_off} m off it"),
    )
}

/// S3: a jump's mark stands on the walkmesh under its x and y, whatever
/// its own height: one 3 m over the floor or 2 m under it lands on the
/// floor there, as K1's safe-location search puts it.
fn jump_mark_on_walkmesh() -> Found {
    const MARK: [f32; 2] = [-3.0, 2.0];
    let mut off = Vec::new();
    for lift in [3.0, -2.0] {
        let (mut ground, walker) = walking_ground([-2.0, -3.0], false)?;
        let [x, y, z] = floor_at(MARK);
        let jump = Queue::Jump {
            to: Target::Point([x, y, z + lift], 0.0),
        };
        walked(&mut ground, walker, jump, 200);
        off.push(between(ground.at(walker), floor_at(MARK)));
    }
    check(
        off.iter().all(|off| *off <= 0.05),
        format!("{off:.2?} m from the floor under the marks"),
    )
}

/// S3: the same walk with no conversation goes through move_body, never
/// carried, at KOTOR's walk all the same (free play's pace,
/// `walk::paced`).
fn free_play_unchanged() -> Found {
    let (mut ground, walker) = walking_ground(WALK_FROM, false)?;
    // 8 s as the held walk's: the stage's creatures, settled apart as the
    // area's loader stands them, leave one beside the walker's start,
    // which its walk goes round first.
    let track = walked(&mut ground, walker, move_to(WALK_TO, false, None), 8000);
    let speed = track.speed(WALK_FROM, WALK_TO, (2.0, 8.0));
    let carried = track.carried.iter().any(|carried| *carried);
    check(
        speed.is_some_and(|speed| (speed - KOTOR_WALK).abs() <= KOTOR_WALK * 0.05) && !carried,
        format!("walked at {speed:?} m/s (KOTOR's {KOTOR_WALK:.2}), carried {carried}"),
    )
}

/// S3: the conversation ends halfway through the walk: the next step is
/// move_body's, the body not jumping (under 0.05 m), and the walk goes on at
/// KOTOR's walk.
fn ends_mid_walk() -> Found {
    let (mut ground, walker) = walking_ground(WALK_FROM, true)?;
    let carried = walked(&mut ground, walker, move_to(WALK_TO, false, None), 3000);
    let was_carried = carried.carried.iter().filter(|carried| **carried).count();
    for _ in 0..8 {
        if !ground.held() {
            break;
        }
        for reply in &mut ground.replies {
            *reply = answer(*reply, Pick::Reply(GOODBYE));
        }
        ground.step();
    }
    let held = ground.held();
    // The first step of the world let go.
    let before = ground.at(walker);
    let mut after = Track::default();
    for _ in 0..steps(1500) {
        ground.step();
        after.record(&ground, walker);
    }
    let jump = between(before, after.at[0]);
    let first = after.carried[0];
    let on = along(WALK_FROM, WALK_TO, after.at[0]);
    let speed = after.speed(WALK_FROM, WALK_TO, (on + 0.5, on + 2.5));
    check(
        !held
            && was_carried > 0
            && jump < 0.05
            && !first
            && speed.is_some_and(|speed| (speed - KOTOR_WALK).abs() <= KOTOR_WALK * 0.05),
        format!(
            "held {held}, carried {was_carried} steps, then a step of {jump:.3} m, carried \
             {first}, then at {speed:?} m/s"
        ),
    )
}

/// S3: the host's body walked by a script along the open gate that leads
/// out, its stride onto the doorway, asks to leave no more than a move_body
/// walk beside it would: no ready check, no module change.
fn walk_asks_no_exit() -> Found {
    let mut ground = Ground::staged(
        1,
        Stage {
            way_out: true,
            ..Stage::default()
        },
    )?;
    let host = ground.body(0)?;
    ground.put(host, [-3.0, 0.0]);
    ground.place(TALKER, TALKER_AT)?;
    ground.step();
    ground.cutscene()?;
    let mut track = Track::default();
    let mut asked = false;
    ground.queue(host, move_to([-0.9, 0.0], false, None));
    for _ in 0..steps(3000) {
        ground.step();
        track.record(&ground, host);
        asked |= ground.last.ready.is_some() || ground.last.transition.is_some();
    }
    let carried = track.carried.iter().filter(|carried| **carried).count();
    let off = between(ground.at(host), floor_at([-0.9, 0.0]));
    check(
        carried > 0 && off <= 0.1 && !asked,
        format!("carried {carried} steps, {off:.3} m off the end, asked to leave {asked}"),
    )
}

/// The party leaving the module, by a script (`StartNewModule`) or through
/// the open gate leading out (the ready check's ask): the area's `OnExit`
/// and the module's `Mod_OnClientLeav` run once each, in the step the
/// transition is asked, before the module goes.
fn leaving_heard() -> Found {
    let runs = |ground: &Ground| {
        let report = ground.world.script_report();
        ["probe_area_exit", "probe_module_leave"]
            .map(|script| report.runs.get(script).copied().unwrap_or(0))
    };
    let stage = || Stage {
        leaving: true,
        way_out: true,
        ..Stage::default()
    };
    let mut scripted = Ground::staged(1, stage())?;
    scripted.run_script("probe_leave", TALKER)?;
    let by_script = (runs(&scripted), scripted.last.transition.is_some());
    let mut walked = Ground::staged(1, stage())?;
    let host = walked.body(0)?;
    walked.put(host, [0.0, 0.0]);
    let mut by_exit = ([0; 2], false);
    for _ in 0..steps(1000) {
        walked.step();
        if walked.last.transition.is_some() {
            by_exit = (runs(&walked), true);
            break;
        }
    }
    walked.idle(200);
    let after = runs(&walked);
    check(
        by_script == ([1, 1], true) && by_exit == ([1, 1], true) && after == [1, 1],
        format!(
            "(OnExit, Mod_OnClientLeav) runs as asked: by a script {by_script:?}, through \
             the gate {by_exit:?}, then {after:?}"
        ),
    )
}

/// An item the party comes to hold runs the module's acquire hook once,
/// which sees the first PC as the one who came by it (`GetEnteringObject`,
/// as TSL's 402DXN `k_check_item` asks for the zakkeg's ear) and the item
/// as acquired (`GetModuleItemAcquired`, K1's); and the same item gone from
/// it its unacquire hook once (`Mod_OnUnAqreItem`).
fn item_lost_heard() -> Found {
    use crate::inventory::{FIRST_ITEM, Item, PARTY};
    let mut ground = Ground::staged(
        1,
        Stage {
            leaving: true,
            ..Stage::default()
        },
    )?;
    let runs = |ground: &Ground| {
        let report = ground.world.script_report();
        ["probe_acquired", "probe_lost"].map(|script| report.runs.get(script).copied().unwrap_or(0))
    };
    let id = FIRST_ITEM + 77;
    let mut state = ground.world.script_state().clone();
    state.items.insert(
        id,
        Item {
            template: "probe_item".into(),
            holder: PARTY,
            slot: None,
            stack: 1,
            charges: 0,
            droppable: true,
            plot: false,
            identified: true,
            infinite: false,
            non_equippable: false,
            upgrades: 0,
            fitted: Vec::new(),
        },
    );
    ground.world.set_script_state(state);
    ground.idle(100);
    let gained = runs(&ground);
    // Its run saw the first PC come by it (`GetEnteringObject`) and the
    // item itself (`GetModuleItemAcquired`).
    let told = [ACQUIRED_BY_PC, ACQUIRED_ITEM]
        .map(|name| ground.world.script_state().booleans.get(name).copied());
    let mut state = ground.world.script_state().clone();
    state.items.remove(&id);
    ground.world.set_script_state(state);
    ground.idle(100);
    let lost = runs(&ground);
    check(
        gained == [1, 0] && lost == [1, 1] && told == [Some(true), Some(true)],
        format!(
            "(acquired, lost) runs: after the gain {gained:?}, after the loss {lost:?}; \
             (by the first PC, the item named) {told:?}"
        ),
    )
}

/// The ground staged with the mesh through the gate's doorway (`Stage::
/// through`), a party conversation holding the world, the gate shut.
fn through_ground() -> Result<Ground, String> {
    let mut ground = Ground::staged(
        1,
        Stage {
            walker: true,
            through: true,
            ..Stage::default()
        },
    )?;
    let host = ground.body(0)?;
    ground.put(host, HOST_AT);
    ground.place(TALKER, TALKER_AT)?;
    ground.step();
    ground.cutscene()?;
    Ok(ground)
}

/// Where a walk through the gate starts and ends (the gate's panel 2 m on),
/// and how far short of the panel a body's front stands.
const BEFORE_GATE: [f32; 2] = [-2.0, 0.0];
const PAST_GATE: [f32; 2] = [2.0, 0.0];
const HALF_BODY_M: f32 = crate::motion::HALF_WIDTH * UNIT;

/// `who`, on the ground `through_ground` stages, walked through the shut
/// gate (`ActionMoveToLocation`) for `ms`: its track, how far on of the
/// gate's panel its front came while the gate was shut (m, negative short
/// of it), and whether the gate opened.
fn through_gate(ground: &mut Ground, who: SimId, ms: u64) -> (Track, f32, bool) {
    ground.queue(who, move_to(PAST_GATE, false, None));
    let (mut track, mut past_shut, mut opened) = (Track::default(), f32::MIN, false);
    for _ in 0..steps(ms) {
        ground.step();
        track.record(ground, who);
        let front = along(BEFORE_GATE, PAST_GATE, ground.at(who)) + HALF_BODY_M - 2.0;
        if ground.gate_shut() {
            past_shut = past_shut.max(front);
        } else {
            opened = true;
        }
    }
    (track, past_shut, opened)
}

/// S3: a creature's walk through the shut gate, the world held, stops short
/// of its panel and opens it, then walks on through it and arrives.
fn walk_opens_door() -> Found {
    let mut ground = through_ground()?;
    let walker = ground.place(WALKER, BEFORE_GATE)?;
    ground.step();
    let (track, past, opened) = through_gate(&mut ground, walker, 5000);
    let off = between(ground.at(walker), floor_at(PAST_GATE));
    let carried = track.carried.iter().filter(|carried| **carried).count();
    check(
        opened && past <= 0.0 && off <= 0.1 && carried > 0,
        format!(
            "the gate opened {opened}, the front {past:.3} m past the shut panel at most, {off:.3} m off the end, carried {carried} steps"
        ),
    )
}

/// A creature walking to the shut gate opens the doors it runs into
/// (`movement::opens_doors`); killed on the way, its walk not yet cleared,
/// it opens none (the Endar Spire's end_door10_cut2, opened by a Sith
/// killed as his walk met it: its scene, kept for a party member's
/// opening, never played).
fn dead_open_no_door() -> Found {
    let mut ground = through_ground()?;
    let walker = ground.place(WALKER, BEFORE_GATE)?;
    ground.step();
    ground.queue(walker, move_to(PAST_GATE, false, None));
    for _ in 0..steps(500) {
        ground.step();
    }
    let mut fighters = ground.world.world.query::<(&Id, &mut Fighting)>();
    let (_, mut fighting) = fighters
        .iter_mut(&mut ground.world.world)
        .find(|(id, _)| id.0 == walker)
        .ok_or("no walker")?;
    let walking = super::super::movement::opens_doors(&fighting.0);
    fighting.0.life = crate::combat::fighter::Life::Dead;
    let dead = super::super::movement::opens_doors(&fighting.0);
    check(
        walking && !dead,
        format!("opens doors walking {walking}, killed on its walk {dead}"),
    )
}

/// A creature whose walk runs into the shut gate hears its `OnBlocked`
/// (`probe_blocked`), the gate its blocking door (`GetBlockingDoor`), as
/// the gate opens for it: once, the gate not shut again.
fn blocked_by_door() -> Found {
    let mut ground = through_ground()?;
    let walker = ground.place(WALKER, BEFORE_GATE)?;
    let gate = ground.object(GATE)?;
    ground.step();
    let (_, _, opened) = through_gate(&mut ground, walker, 3000);
    let state = ground.world.script_state();
    let door = state
        .last
        .get(&(walker.0, crate::script::last::BLOCKING_DOOR))
        .copied();
    let report = ground.world.script_report();
    let runs = report.runs.get("probe_blocked").copied().unwrap_or(0);
    check(
        opened && runs == 1 && door == Some(gate.0),
        format!(
            "the gate opened {opened}; OnBlocked ran {runs} times, the blocking door {door:?} (the gate {})",
            gate.0
        ),
    )
}

/// S3: the companion's and the host's walks through the shut gate, the
/// world held (the party opens nothing, S4): neither crosses the panel nor
/// opens it, and each move ends.
fn party_stops_at_door() -> Found {
    let mut found = Vec::new();
    for party in [BUDDY, u32::MAX] {
        let mut ground = through_ground()?;
        let who = match party {
            u32::MAX => ground.body(0)?,
            index => ground.object(index)?,
        };
        ground.put(who, BEFORE_GATE);
        ground.step();
        let (track, past, opened) = through_gate(&mut ground, who, 3000);
        found.push((track.ended().is_some(), past, opened));
    }
    check(
        found
            .iter()
            .all(|&(ended, past, opened)| ended && past <= 0.0 && !opened),
        format!("(companion, host): (move ended, front past the panel m, gate opened) {found:?}"),
    )
}

/// S3: a held walk from where the mesh has no face (beyond its far side,
/// the gate shut between) ends, never walked on for good.
fn walk_off_mesh_ends() -> Found {
    let (mut ground, walker) = walking_ground([0.8, 1.5], true)?;
    let track = walked(
        &mut ground,
        walker,
        move_to([-3.0, -1.0], false, None),
        10_000,
    );
    let ended = track.ended();
    let carried = track.carried.iter().filter(|carried| **carried).count();
    check(
        ended.is_some(),
        format!("ended at step {ended:?}, carried {carried} steps"),
    )
}

/// S3: the companion walked (the world held, so it opens nothing) to the
/// shut gate's doorway itself, the route's last face a door's: it stops
/// short of the panel, and its move ends.
fn door_goal_ends() -> Found {
    let mut ground = through_ground()?;
    let buddy = ground.place(BUDDY, BEFORE_GATE)?;
    ground.step();
    let track = walked(&mut ground, buddy, move_to([-0.3, 0.5], false, None), 4000);
    let front = along(BEFORE_GATE, PAST_GATE, ground.at(buddy)) + HALF_BODY_M - 2.0;
    let ended = track.ended();
    let carried = track.carried.iter().filter(|carried| **carried).count();
    check(
        ended.is_some() && front <= 0.0 && ground.gate_shut(),
        format!(
            "ended at step {ended:?}, its front {front:.3} m past the panel, carried {carried} steps"
        ),
    )
}

/// S3: a held forced walk (no queue rule ends it before its 30 s) from
/// where the mesh has no face (beyond the wall from it, the companion, who
/// opens nothing while held) is stepped by move_body against the wall: its walk
/// stalls and is stuck (`STUCK_STALLS`), as KOTOR's walker never carried it.
fn stepped_walk_stalls() -> Found {
    let (mut ground, _) = walking_ground(WALK_FROM, true)?;
    let buddy = ground.place(BUDDY, [0.8, 2.0])?;
    ground.step();
    let track = walked(
        &mut ground,
        buddy,
        move_to([-3.0, -1.0], false, Some(30_000)),
        10_000,
    );
    let stalls = track.stalls.iter().flatten().max().copied();
    let carried = track.carried.iter().filter(|carried| **carried).count();
    check(
        stalls.is_some_and(|stalls| stalls >= crate::script::STUCK_STALLS),
        format!("stalls up to {stalls:?} in 10 s, carried {carried} steps"),
    )
}

/// A script's walk out of a scene along a strip of mesh that runs on
/// through a wall (a walkmesh under a wall's foot): move_body's box stops at
/// the wall, and the walk goes on on KOTOR's walker within a few steps, not
/// after a second with no headway (`world::move_actors`; Peragus's Sith
/// troopers boarding the Ebon Hawk, 107PER, stood a second at each gap
/// between a barrel and a crate's edge), and comes out past the wall.
fn wedged_walk_goes_on_at_once() -> Found {
    let stage = Stage {
        over_wall: true,
        ..Stage::default()
    };
    let (from, to) = ([-3.0, 4.0], [1.5, 4.0]);
    // The wall stands 3 m along; the walk ends within its range of the goal.
    const WALL_ALONG: f32 = 3.0;
    let (mut ground, walker) = walking_on(stage, from, false)?;
    let track = walked(&mut ground, walker, move_to(to, false, None), 6000);
    let stopped =
        (1..track.at.len()).find(|&step| between(track.at[step - 1], track.at[step]) < 0.0005);
    let carried = track.carried.iter().position(|carried| *carried);
    let waited = stopped
        .zip(carried)
        .map(|(stopped, carried)| carried.saturating_sub(stopped));
    let past = along(from, to, ground.at(walker));
    check(
        waited.is_some_and(|n| n < steps(rules::walking::route::STALL_MS) / 4) && past > WALL_ALONG,
        format!("stopped at step {stopped:?}, carried from {carried:?}, {past:.2} m along"),
    )
}

/// S3: a held jump to a mark with no free spot within 20 m (one 40 m out
/// past the ground's wall, over no floor) fails: the jumper stays put.
fn jump_fails_held() -> Found {
    jump_fails(true)
}

/// The same with no conversation.
fn free_jump_fails() -> Found {
    jump_fails(false)
}

fn jump_fails(held: bool) -> Found {
    let (mut ground, walker) = walking_ground([-2.0, -3.0], held)?;
    let before = ground.at(walker);
    let jump = Queue::Jump {
        to: Target::Point(floor_at([-2.0, 40.0]), 0.0),
    };
    let track = walked(&mut ground, walker, jump, 200);
    let moved = between(before, ground.at(walker));
    let ended = track.ended();
    check(
        moved < 0.05 && ended.is_some(),
        format!("moved {moved:.3} m, the jump ended at step {ended:?}"),
    )
}

/// S3: a walk that goes nowhere stalls whatever moves it: one KOTOR's
/// walker carries a step toward its corner and move_body takes back, step after
/// step (carried onto the edge of a face the mesh leaves out, slid back),
/// keeps its headway only as it gets nearer, and so stalls within 2 s.
fn alternating_walk_stalls() -> Found {
    let ground = Ground::new(1)?;
    let level = ground.world.level();
    let stand = |at: [f32; 2]| {
        let [x, y, z] = floor_at(at).map(|v| v / UNIT);
        crate::motion::MotionState::standing([x, y, z + 0.125 - crate::motion::SOLE_Z], 0.0)
    };
    let (back, on) = (stand([-4.0, -3.0]), stand([-4.0, -2.99]));
    let mut walker = crate::walk::Walker::new(stand([-4.0, 3.0]).origin);
    let doors = |_| crate::snapshot::ObjectState::default();
    let mut stalls = 0;
    for step in 0..steps(2000) {
        let now = step as u64 * STEP_MS;
        // Carried on (a hair nearer) every other step, taken back between.
        let ps = if step % 2 == 0 { &on } else { &back };
        if step % 2 == 1 {
            walker.keep_headway(now, ps);
        }
        walker.command(level, ps, now, &doors);
        stalls = stalls.max(walker.stalls);
    }
    check(stalls > 0, format!("stalls in 2 s: {stalls}"))
}

/// The ground staged with the mesh through the gate's doorway (`Stage::
/// through`), the gate opened by the companion and the companion put aside
/// (the corner by the wedge), then a party conversation holding the world.
fn open_gate_ground() -> Result<Ground, String> {
    let mut ground = Ground::staged(
        1,
        Stage {
            through: true,
            ..Stage::default()
        },
    )?;
    let host = ground.body(0)?;
    ground.put(host, HOST_AT);
    ground.place(TALKER, TALKER_AT)?;
    let buddy = ground.place(BUDDY, [-1.2, 0.8])?;
    ground.run_script("probe_open", BUDDY)?;
    ground.idle(1000);
    if ground.gate_shut() {
        return Err("the companion opened no gate".into());
    }
    ground.put(buddy, [-7.0, 5.0]);
    ground.step();
    ground.cutscene()?;
    Ok(ground)
}

/// S3: the host's body walked by a script, the world held, stepped as a
/// client predicts it from a snapshot, lands where the authority puts it:
/// on a straight walk, and on one through the open gate, whose route turns
/// at the doorway's corners short of its goal (the client goes on from
/// each, as the authority does).
fn predicted_matches() -> Found {
    let (straight, _) = walking_ground([-6.0, 4.0], true)?;
    // The ground, where it walks from and to, how long the authority walks
    // it first and how long the client then predicts it (ms; short of the
    // goal, where the authority's queue ends the move within 0.1 m and a
    // client walks on to the spot), and the fewest corners its route must
    // still turn at.
    let cases = [
        ("straight", straight, (WALK_FROM, WALK_TO), (1000, 500), 0),
        (
            "through the doorway",
            open_gate_ground()?,
            ([-2.0, -4.0], [2.0, -4.0]),
            (100, 1800),
            1,
        ),
    ];
    let mut seen = Vec::new();
    for (name, mut ground, (from, to), (walk_ms, predict_ms), turns) in cases {
        let host = ground.body(0)?;
        ground.put(host, from);
        walked(&mut ground, host, move_to(to, false, None), walk_ms);
        // The corners its route still turns at before the goal.
        let walker = ground
            .entity(host)
            .and_then(|entity| entity.fighter)
            .and_then(|fighter| fighter.mind.walker);
        let doors = |_| crate::snapshot::ObjectState::default();
        let corners = walker
            .zip(ground.ps(host))
            .and_then(|(walker, ps)| {
                crate::walk::route(ground.world.level(), ps.origin, walker.goal, &doors)
            })
            .map_or(0, |corners| corners.len().saturating_sub(1));
        let mut client = ground.predicted()?;
        let mut worst: f32 = 0.0;
        for _ in 0..steps(predict_ms) {
            ground.step();
            client.step();
            worst = worst.max(between(ground.at(host), client.at(host)));
        }
        let gone = between(ground.at(host), floor_at(from));
        let fits = gone > 1.0 && worst < 0.001 && corners >= turns;
        seen.push(format!(
            "{name}: {fits} (the host {gone:.2} m on past {corners} corners, the client at \
             most {worst:.4} m off)"
        ));
    }
    check(
        seen.iter().all(|line| line.contains(": true")),
        seen.join("; "),
    )
}

/// S3: a walk saved halfway and loaded: the loaded one finishes on the same
/// step, at the same point.
fn saved_mid_walk() -> Found {
    let (mut ground, walker) = walking_ground(WALK_FROM, true)?;
    walked(&mut ground, walker, move_to(WALK_TO, false, None), 3000);
    let mut loaded = ground.reloaded()?;
    let finish = |ground: &mut Ground| {
        let mut track = Track::default();
        for _ in 0..steps(5000) {
            ground.step();
            track.record(ground, walker);
        }
        (track.ended(), ground.at(walker))
    };
    let (saved, there) = finish(&mut ground);
    let (load, here) = finish(&mut loaded);
    check(
        saved.is_some_and(|step| step > 10) && saved == load && between(there, here) < 1e-4,
        format!(
            "ended at step {saved:?} and {load:?}, {:.5} m apart",
            between(there, here)
        ),
    )
}
