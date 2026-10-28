//! The probes of `coop_talk` (`kjk sim probes`).

use super::super::{Clock, Fighting, Id, Player};
use super::*;
use crate::action::Action;
use crate::combat::fighter::Fighter;
use crate::combat::mind;
use crate::input::{Command, Seat, TickInput, buttons};
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::motion::{self};
use crate::notice::{Notice, Screen};
use crate::pazaak::{Match, Move, Phase as Stage};
use crate::probe::check;
use crate::script::dialogue::{Conversation, Line, Pick, Start, answer};
use crate::script::dialogue_probes::talking;
use crate::snapshot::{Busy, Kind, RequestWait, SimId, Snapshot, SnapshotHash};
use crate::world::{StepReason, WorldConfig};
use collision::UNIT;
use fmt_odyssey::dlg::Dlg;
use fmt_odyssey::ncs::{Const, Ncs, Op, Pair, Ty};
use script_ncs::vm::ObjectId;
use std::collections::BTreeMap;

/// A probe's name and what it found.
type Case = (&'static str, Result<String, String>);
/// One part of the scenario; each starts and ends with no conversation.
type Phase = fn(&mut Run) -> Result<Vec<Case>, String>;

/// The parts, in order.
const PHASES: &[Phase] = &[
    marks,
    guest_click,
    store,
    store_for_running_conversation,
    shops,
    pazaak,
    held,
    bark,
    screens,
    script_talk,
    private,
    cutscenes,
    requests,
    leaving,
    interjections,
    interruptions,
    matches,
    match_rules,
    watch,
    watch_more,
    watch_held,
    exits,
    endings,
];

/// The seats: the host, a guest driving the companion, a guest watching
/// (until `private` gives it the second companion).
const SEATS: u8 = 3;
/// The companion's number among the objects. The proving ground has two
/// of its own, then the probes' placements: the one who talks 2, the one
/// who barks 3, the companion 4, the merchant 5, its store 6, the one whose
/// dialogue hook begins its conversation 7, the dealer 8, the second
/// companion 9, the companion no seat drives 10, the console 11, the one
/// whose conversation is a cutscene 12, the lever 13, the one no hurt
/// listener interrupts 14.
const BUDDY: u32 = 4;
const BUDDY2: u32 = 9;
const AIDE: u32 = 10;
const CONSOLE: u32 = 11;
const CUTTER: u32 = 12;
const LEVER: u32 = 13;
const CAPTIVE: u32 = 14;
/// The one no hurt listener interrupts (`Interruptable` 0).
const CAPTIVE_AT: [f32; 2] = [2.0, 3.0];
const BUDDY2_AT: [f32; 2] = [-3.0, 3.5];
const AIDE_AT: [f32; 2] = [-1.0, 4.5];
/// The console, and where a user stands to use it.
const CONSOLE_AT: [f32; 2] = [2.0, -2.0];
const CONSOLE_FROM: [f32; 2] = [1.0, -2.0];
/// The one whose conversation is a cutscene (`Skippable` 0).
const CUTTER_AT: [f32; 2] = [-2.0, -2.0];
const CUT_FROM: [f32; 2] = [-1.0, -2.0];
/// The lever, and where a user stands to use it.
const LEVER_AT: [f32; 2] = [2.0, -3.0];
const LEVER_FROM: [f32; 2] = [1.0, -3.0];
/// Second places to use the one who talks, and the one whose dialogue
/// hook begins its conversation, from: beside them.
const TALK_BESIDE: [f32; 2] = [2.0, -1.0];
const HOOK_BESIDE: [f32; 2] = [1.0, -3.0];
/// Where they stand, and where a user stands to use them (metres from
/// the entry, across the floor).
const TALKER_AT: [f32; 2] = [2.0, 0.0];
/// The one who deals pazaak (see `BUDDY`), and where it stands.
const DEALER: u32 = 8;
const DEALER_AT: [f32; 2] = [-2.0, 3.0];
const DEAL_FROM: [f32; 2] = [-1.0, 3.0];
/// Where a player stands to use a body at `DEAL_FROM` (R9a).
const WATCH_FROM: [f32; 2] = [0.0, 3.0];
/// A second place to use the dealer from: beside it, facing north.
const DEAL_BESIDE: [f32; 2] = [-2.0, 2.0];
/// The talker's replies: "Again." and "Goodbye." anyone takes, "Join me."
/// (it adjusts the PC's alignment) the story's.
const AGAIN: u8 = 0;
const GOODBYE: u8 = 1;
const JOIN: u8 = 2;
const TALK_FROM: [f32; 2] = [1.0, 0.0];
const BARKER_AT: [f32; 2] = [-2.0, 0.0];
const MERCHANT_AT: [f32; 2] = [0.0, 2.0];
const STORE_AT: [f32; 2] = [0.0, 3.0];
const BUDDY_AT: [f32; 2] = [0.0, -1.5];
/// The one who barks: its number among the objects (see `BUDDY`).
const BARKER: u32 = 3;
const BARK_FROM: [f32; 2] = [-1.0, 0.0];
/// Beyond the wall across, farther from `TALK_FROM` and `HOST_ASIDE`
/// than a conversation gathers from (11 m and more).
const FAR: [f32; 2] = [11.9, 2.0];
const FAR_TOO: [f32; 2] = [6.3, 10.3];
/// The one who talks, and the merchant, among the objects (see `BUDDY`).
const TALKER: u32 = 2;
const MERCHANT: u32 = 5;
/// The store's number among the objects (see `BUDDY`).
const STORE: u32 = 6;
const SHOP_FROM: [f32; 2] = [0.0, 1.0];
/// A second place to use the merchant from: beside it.
const SHOP_BESIDE: [f32; 2] = [1.0, 2.0];
/// The proving ground's way out among the objects (see `BUDDY`): a trigger
/// to `elsewhere`.
const WAY_OUT: u32 = 1;
/// What the store sells in `shops` (`catalog`), and its price there.
const CELL: &str = "probe_cell";
const CELL_COST: u32 = 10;
/// The one whose dialogue hook begins its conversation (`probe_begin`).
const HOOKED: u32 = 7;
const HOOKED_AT: [f32; 2] = [0.0, -3.0];
const HOOK_FROM: [f32; 2] = [-1.0, -3.0];
/// Where the host stands aside while the guest uses something.
const HOST_ASIDE: [f32; 2] = [-1.0, -1.0];
/// Steps between two uses by one body (`USE_EVERY_MS` over 8 ms steps).
const USE_GAP: usize = 13;
/// A step's length (ms).
const STEP_MS: u64 = 8;

/// How many steps last `ms`.
fn steps(ms: u64) -> usize {
    (ms / STEP_MS) as usize
}
/// How long after its line `probe_later` deals (s).
const LATER_S: f32 = 1.0;
/// The custom token `probe_who` sets: `GetIsPC(GetPCSpeaker())`.
const WHO_TOKEN: i32 = 7;
/// The custom token `probe_talk`'s `EndConverAbort` sets to 1.
const ABORT_TOKEN: i32 = 8;
/// The custom token `probe_busy` sets: `GetIsInConversation(GetFirstPC())`.
const BUSY_TOKEN: i32 = 9;

/// The co-op conversation probes (`kjk sim probes`): every phase's cases,
/// one seat alone, and the whole scenario run twice ending in the same
/// state.
pub fn probes() -> Vec<Case> {
    let again = scenario().map(|(_, hash)| hash);
    let (mut cases, hash) = match scenario() {
        Ok(ran) => ran,
        Err(why) => return vec![("coop_talk_scenario", Err(why))],
    };
    cases.extend(one_seat_as_before().unwrap_or_else(|why| vec![("one_seat_as_before", Err(why))]));
    cases.extend(one_seat_leading().unwrap_or_else(|why| vec![("one_seat_leading", Err(why))]));
    cases.extend(
        one_seat_no_watchers().unwrap_or_else(|why| vec![("one_seat_no_watchers", Err(why))]),
    );
    cases.extend(
        fight_near_not_pulled_in()
            .unwrap_or_else(|why| vec![("fight_near_not_pulled_in", Err(why))]),
    );
    cases.push((
        "same_every_run",
        check(
            again.as_ref().is_ok_and(|again| *again == hash),
            format!("{} then {:?}", hash.short(), again.map(|h| h.short())),
        ),
    ));
    cases
}

fn scenario() -> Result<(Vec<Case>, SnapshotHash), String> {
    let mut run = Run::new(SEATS, Some(Seat(1)))?;
    let mut cases = Vec::new();
    for phase in PHASES {
        cases.extend(phase(&mut run)?);
    }
    Ok((cases, run.last.hash()))
}

/// The scenario's world and what it keeps between steps.
struct Run {
    world: super::super::SimWorld,
    tick: u64,
    /// The proving ground's entry (metres): everyone is placed from it.
    entry: [f32; 3],
    seats: u8,
    /// Each seat's answer byte, held as a client holds it.
    replies: Vec<u8>,
    /// Seats sending no command yet (a late joiner).
    silent: Vec<u8>,
    last: Snapshot,
    /// Every notice, with the tick it came in.
    notices: Vec<(u64, Seat, Notice)>,
    /// How it steps: as the authority, or as a client predicting.
    reason: StepReason,
}

impl Run {
    /// The proving ground with the probes' placements and conversations,
    /// for `seats` seats: seat 0's avatar, the companion in the party
    /// driven by `driver` (as a guest who joined drives one), seats past
    /// the second silent until they join with no body.
    fn new(seats: u8, driver: Option<Seat>) -> Result<Run, String> {
        Run::on_ground(seats, driver, |_| {})
    }

    /// `new`, the ground's module as `tweak` leaves it.
    fn on_ground(
        seats: u8,
        driver: Option<Seat>,
        tweak: impl FnOnce(&mut crate::module::ModuleData),
    ) -> Result<Run, String> {
        let ground = crate::determinism::proving_ground();
        let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
        tweak(&mut data);
        let entry = data.entry.position;
        let at = |[x, y]: [f32; 2]| [entry[0] + x, entry[1] + y, entry[2]];
        data.placements.extend(
            [
                creature("talker", at(TALKER_AT), 180.0, "probe_talk"),
                creature("barker", at(BARKER_AT), 0.0, "probe_bark"),
                creature("buddy", at(BUDDY_AT), 90.0, ""),
                creature("merchant", at(MERCHANT_AT), 270.0, "probe_shop"),
                ObjectData {
                    kind: ObjectKind::Store,
                    useable: false,
                    store: Some(crate::inventory::Terms {
                        markup: 100,
                        markdown: 50,
                        sells: true,
                        buys: true,
                    }),
                    ..creature("probe_store", at(STORE_AT), 0.0, "")
                },
                ObjectData {
                    scripts: vec![("ScriptDialogue".into(), "probe_begin".into())],
                    ..creature("hooked", at(HOOKED_AT), 180.0, "")
                },
                creature("dealer", at(DEALER_AT), 180.0, "probe_deal"),
                creature("buddy2", at(BUDDY2_AT), 90.0, ""),
                creature("aide", at(AIDE_AT), 90.0, ""),
                ObjectData {
                    kind: ObjectKind::Placeable,
                    scripts: vec![("OnUsed".into(), "probe_console".into())],
                    ..creature("console", at(CONSOLE_AT), 180.0, "")
                },
                creature("cutter", at(CUTTER_AT), 0.0, "probe_cut"),
                ObjectData {
                    kind: ObjectKind::Placeable,
                    scripts: vec![("OnUsed".into(), "probe_lever".into())],
                    ..creature("lever", at(LEVER_AT), 180.0, "")
                },
                ObjectData {
                    interruptable: false,
                    ..creature("captive", at(CAPTIVE_AT), 180.0, "probe_talk")
                },
            ]
            .map(Placement::Own),
        );
        let level = ground.with_module(data).with_scripts(library());
        // Only the host has an avatar.
        let config = WorldConfig {
            seed: 1,
            seats,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: crate::combat::Sheet::dev_soldier(),
            guests: crate::Guests::Companions,
        };
        let mut world =
            super::super::SimWorld::new(config, level).map_err(|error| error.to_string())?;
        let last = world.snapshot();
        let mut run = Run {
            world,
            tick: 0,
            entry,
            seats,
            replies: vec![0; usize::from(seats)],
            silent: (2..seats).collect(),
            last,
            notices: Vec::new(),
            reason: StepReason::Authority,
        };
        run.follow(&[(BUDDY, driver)], Some(Seat(0)))?;
        run.step(&[]);
        Ok(run)
    }

    /// The world again from its last snapshot with each placement of
    /// `members` led by `leader` (none: out of the party), and driven by
    /// its seat (none: its own mind).
    fn follow(
        &mut self,
        members: &[(u32, Option<Seat>)],
        leader: Option<Seat>,
    ) -> Result<(), String> {
        self.rebuilt(|snapshot| {
            for &(index, driver) in members {
                let id = object(snapshot, index)?;
                let fighter = snapshot
                    .entities
                    .iter_mut()
                    .find(|entity| entity.id == id)
                    .and_then(|entity| entity.fighter.as_mut())
                    .ok_or(format!("object {index} does not fight"))?;
                fighter.mind.leader = leader;
                fighter.mind.driver = driver;
                fighter.faction = Some(crate::combat::sheet::factions::PLAYER);
            }
            Ok(())
        })
    }

    /// The world again from its last snapshot as `change` leaves it, the
    /// scripts' state kept.
    fn rebuilt(
        &mut self,
        change: impl FnOnce(&mut Snapshot) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut snapshot = self.last.clone();
        change(&mut snapshot)?;
        let scripts = self.world.script_state().clone();
        let level = self.world.level().clone();
        self.world = super::super::SimWorld::from_snapshot(&snapshot, level)
            .map_err(|error| error.to_string())?;
        self.world.set_script_state(scripts);
        self.last = snapshot;
        Ok(())
    }

    /// One step: each seat but the silent sends `with`'s command for it,
    /// else stands still, with its held answer byte.
    fn step(&mut self, with: &[(u8, Command)]) {
        self.step_acting(with, TickInput::default());
    }

    /// One step with `input`'s actions, as `step` sends `with`.
    fn step_with(&mut self, input: TickInput) {
        self.step_acting(&[], input);
    }

    fn step_acting(&mut self, with: &[(u8, Command)], mut input: TickInput) {
        self.tick += 1;
        for seat in (0..self.seats).filter(|seat| !self.silent.contains(seat)) {
            let command = with
                .iter()
                .find(|(of, _)| *of == seat)
                .map_or_else(Command::default, |(_, command)| *command);
            let reply = self.replies[usize::from(seat)];
            input.insert(Seat(seat), Command { reply, ..command });
        }
        self.last = crate::step(
            &mut self.world,
            self.tick,
            &input,
            STEP_MS as u32,
            self.reason,
        );
        let tick = self.tick;
        self.notices.extend(
            self.world
                .take_notices()
                .into_iter()
                .map(|(seat, notice)| (tick, seat, notice)),
        );
    }

    /// The custom tokens forgotten, so a phase reads only its own.
    fn forget_tokens(&mut self) {
        let mut state = self.world.script_state().clone();
        state.talk.tokens.clear();
        self.world.set_script_state(state);
    }

    fn idle(&mut self, steps: usize) {
        for _ in 0..steps {
            self.step(&[]);
        }
    }

    /// Seats answer (each byte's count moves), in one step.
    fn answers(&mut self, picks: &[(u8, Pick)]) {
        self.answers_with(picks, TickInput::default());
    }

    /// `answers` in a step with `input`'s actions.
    fn answers_with(&mut self, picks: &[(u8, Pick)], input: TickInput) {
        for &(seat, pick) in picks {
            let at = usize::from(seat);
            self.replies[at] = answer(self.replies[at], pick);
        }
        self.step_with(input);
    }

    fn body(&self, seat: u8) -> Result<SimId, String> {
        self.last
            .body(Seat(seat))
            .map(|entity| entity.id)
            .ok_or(format!("seat {seat} has no body"))
    }

    /// `who` standing still `off` (metres) from the entry, on its floor.
    fn put(&mut self, who: SimId, off: [f32; 2]) {
        let entry = self.entry;
        let mut players = self.world.world.query::<(&Id, &mut Player)>();
        for (id, mut player) in players.iter_mut(&mut self.world.world) {
            if id.0 == who {
                let ps = &mut player.0;
                ps.origin = [
                    (entry[0] + off[0]) / UNIT,
                    (entry[1] + off[1]) / UNIT,
                    entry[2] / UNIT + 0.125 - motion::SOLE_Z,
                ];
                ps.velocity = [0.0; 3];
            }
        }
    }

    /// The view angles that turn `who` to `yaw` (degrees).
    fn heading(&mut self, who: SimId, yaw: f32) -> [i16; 3] {
        let mut players = self.world.world.query::<(&Id, &Player)>();
        players
            .iter(&self.world.world)
            .find(|(id, _)| id.0 == who)
            .map_or([0; 3], |(_, player)| {
                mind::looking(&player.0, 0.0, yaw).angles
            })
    }

    /// Seat `seat`'s body, standing `off` from the entry, uses what lies
    /// at `yaw`, after the last use's delay.
    fn click(&mut self, seat: u8, off: [f32; 2], yaw: f32) -> Result<(), String> {
        self.clicks(&[(seat, off, yaw)])
    }

    /// Each seat's body uses something as `click` has it, all in one step.
    fn clicks(&mut self, uses: &[(u8, [f32; 2], f32)]) -> Result<(), String> {
        self.idle(USE_GAP);
        self.uses(uses)
    }

    /// `clicks` with no delay first: the use's step is the next.
    fn uses(&mut self, uses: &[(u8, [f32; 2], f32)]) -> Result<(), String> {
        self.uses_with(uses, TickInput::default())
    }

    /// `uses` in a step with `input`'s actions.
    fn uses_with(&mut self, uses: &[(u8, [f32; 2], f32)], input: TickInput) -> Result<(), String> {
        let mut commands = Vec::new();
        for &(seat, off, yaw) in uses {
            let who = self.body(seat)?;
            self.put(who, off);
            let angles = self.heading(who, yaw);
            let using = Command {
                angles,
                buttons: buttons::USE,
                ..Command::default()
            };
            commands.push((seat, using));
        }
        self.step_acting(&commands, input);
        Ok(())
    }

    /// Each seat's body uses something as `clicks` has it, then each picks
    /// the story's reply ("Join me.") in one step: the conversation each
    /// listens in, in the order of `uses`.
    fn ask(&mut self, uses: &[(u8, [f32; 2], f32)]) -> Result<Vec<u32>, String> {
        self.clicks(uses)?;
        let picks: Vec<(u8, Pick)> = uses
            .iter()
            .map(|(seat, ..)| (*seat, Pick::Reply(JOIN)))
            .collect();
        self.answers(&picks);
        uses.iter()
            .map(|&(seat, ..)| {
                let body = self.body(seat)?;
                self.last
                    .talk
                    .conversations
                    .iter()
                    .find(|c| c.listener.0 == body.0)
                    .map(|c| c.id)
                    .ok_or(format!("seat {seat} talks with no one"))
            })
            .collect()
    }

    /// The host leads through placement `index` (`world::lent`: in the
    /// party table, its leader), or through its own character again.
    fn lead(&mut self, index: Option<u32>) -> Result<(), String> {
        const NPC: u8 = 0;
        let mut state = self.world.script_state().clone();
        let table = &mut state.party;
        match index {
            Some(index) => {
                let object = Some(object(&self.last, index)?.0);
                let npc = crate::script::party::Npc {
                    template: String::new(),
                    object,
                    kept: None,
                    template_data: None,
                };
                table.available.insert(NPC, npc);
                table.members = vec![NPC];
                table.leader = Some(NPC);
            }
            None => {
                table.available.remove(&NPC);
                table.members.clear();
                table.leader = None;
            }
        }
        self.world.set_script_state(state);
        self.step(&[]);
        Ok(())
    }

    /// A script of the one whose dialogue hook begins its conversation
    /// begins it: a party conversation, with the host's character as its
    /// PC.
    fn cutscene(&mut self) -> Result<(), String> {
        self.run_script("probe_begin")
    }

    /// The one whose dialogue hook begins its conversation runs `script`,
    /// in the next step.
    fn run_script(&mut self, script: &str) -> Result<(), String> {
        let hooked = object(&self.last, HOOKED)?;
        let mut state = self.world.script_state().clone();
        state
            .pending
            .push(crate::script::Pending::new(script, ObjectId(hooked.0)));
        self.world.set_script_state(state);
        self.step(&[]);
        Ok(())
    }

    /// Who is held (moved no farther than a stumble) as every seat runs, by
    /// id, while a conversation is up.
    fn held(&mut self) -> BTreeMap<SimId, bool> {
        self.run_toward(180.0, 25)
            .into_iter()
            .map(|(id, moved)| (id, moved < 0.02))
            .collect()
    }

    /// Every conversation ends: the host, and each seat whose body is a
    /// conversation's listener, answer "Goodbye.", a few times at most.
    fn goodbye(&mut self) {
        for _ in 0..4 {
            let talk = &self.last.talk.conversations;
            if talk.is_empty() {
                return;
            }
            let listening = |seat: &u8| {
                self.last
                    .body(Seat(*seat))
                    .is_some_and(|body| talk.iter().any(|c| c.listener.0 == body.id.0))
            };
            let seats: Vec<(u8, Pick)> = (0..self.seats)
                .filter(|seat| *seat == 0 || listening(seat))
                .map(|seat| (seat, Pick::Reply(GOODBYE)))
                .collect();
            self.answers(&seats);
        }
    }

    /// The host answers the request to join conversation `talk` (R5).
    fn request(&mut self, talk: u32, accept: bool) {
        self.answers_with(&[], requested(talk, accept));
    }

    /// Seat `seat`'s body asks for `dialog` with placement `owner`, as its
    /// use would (R1), the scripts' PC the host's character; begun in the
    /// next step. The conversation it stands in then.
    fn start(&mut self, seat: u8, owner: u32, dialog: &str) -> Result<u32, String> {
        let (body, host) = (self.body(seat)?, self.body(0)?);
        self.begin(owner, (host, body), dialog)?;
        self.last
            .talk
            .holding(ObjectId(body.0))
            .map(|c| c.id)
            .ok_or(format!("seat {seat} began no {dialog}"))
    }

    /// Placement `owner` asked to talk `dialog` with `pc`, `asker`'s use
    /// asking (`INVALID`: a script's), begun in the next step.
    fn begin(
        &mut self,
        owner: u32,
        (pc, asker): (SimId, SimId),
        dialog: &str,
    ) -> Result<(), String> {
        let owner = object(&self.last, owner)?;
        let mut state = self.world.script_state().clone();
        state.talk.asked.push(Start {
            owner: ObjectId(owner.0),
            pc: ObjectId(pc.0),
            asker: ObjectId(asker.0),
            dialog: dialog.into(),
            bark: false,
        });
        self.world.set_script_state(state);
        self.step(&[]);
        Ok(())
    }

    /// Seat `seat` leaves conversation `talk` (R6): whether it is still up
    /// after, and how many leaves were refused for `why` meanwhile.
    fn leave(&mut self, seat: u8, talk: u32, why: &str) -> (bool, usize) {
        let before = refusals(self, why);
        self.answers(&[(seat, Pick::Leave)]);
        (
            self.conversation(talk).is_some(),
            refusals(self, why) - before,
        )
    }

    /// Conversation `id` gone between steps, its scripts not run: one no
    /// seat could answer (two others talking).
    fn forget(&mut self, id: u32) {
        let mut state = self.world.script_state().clone();
        state.talk.conversations.retain(|c| c.id != id);
        self.world.set_script_state(state);
    }

    /// Conversation `id` changed as `change` has it, between steps (a lock
    /// a story step would leave, R6).
    fn edit(&mut self, id: u32, change: impl Fn(&mut Conversation)) {
        let mut state = self.world.script_state().clone();
        state
            .talk
            .conversations
            .iter_mut()
            .filter(|c| c.id == id)
            .for_each(change);
        self.world.set_script_state(state);
    }

    /// The barker turned on the party, fighting (its faction at war with
    /// the player's, struck a moment ago), or back as it was: what puts
    /// the party in combat (`world::InCombat`) and takes it out.
    fn fight(&mut self, on: bool) -> Result<(), String> {
        use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};
        let barker = object(&self.last, BARKER)?;
        let mut state = self.world.script_state().clone();
        let war = crate::module::pair(HOSTILE_1, PLAYER);
        if on {
            state.reputation.insert(war, -50);
        } else {
            state.reputation.remove(&war);
        }
        self.world.set_script_state(state);
        let now = self.now();
        self.fighter(barker, |fighter| {
            fighter.faction = on.then_some(HOSTILE_1);
            fighter.combat_until = if on { now + 60_000 } else { 0 };
        });
        Ok(())
    }

    /// The sim's time now (ms).
    fn now(&self) -> u64 {
        self.world.world.resource::<Clock>().time_ms
    }

    /// `who`'s fighter changed as `change` has it, between steps.
    fn fighter(&mut self, who: SimId, mut change: impl FnMut(&mut Fighter)) {
        let mut fighters = self.world.world.query::<(&Id, &mut Fighting)>();
        for (id, mut fighting) in fighters.iter_mut(&mut self.world.world) {
            if id.0 == who {
                change(&mut fighting.0);
            }
        }
    }

    /// `who` loses a hit point, a damaged event later than the last.
    fn hurt(&mut self, who: SimId) {
        self.idle(steps(super::super::scripts::DAMAGED_EVERY_MS) + 1);
        self.fighter(who, |fighter| fighter.hp -= crate::script::HUNDREDTHS);
        self.step(&[]);
    }

    /// `who` dies, as a script's killing blow leaves it.
    fn kill(&mut self, who: SimId) {
        self.fighter(who, |fighter| {
            fighter.hp = 0;
            fighter.life = crate::combat::fighter::Life::Dead;
        });
        self.step(&[]);
    }

    /// `who` down for a step (I30 P11: no hit points left), then up again
    /// with those it had.
    fn down_a_step(&mut self, who: SimId) {
        let (now, mut had) = (self.now(), 0);
        self.fighter(who, |fighter| {
            had = fighter.hp;
            fighter.fall(now);
        });
        self.step(&[]);
        self.fighter(who, |fighter| fighter.rise(had));
    }

    /// `who` in a fight a while yet (`Fighter::combat_until`), or out of it.
    fn in_combat(&mut self, who: SimId, on: bool) {
        let now = self.now();
        self.fighter(who, |fighter| {
            fighter.combat_until = if on { now + 60_000 } else { 0 };
        });
    }

    /// The match `who` plays, as the scripts keep it.
    fn table(&self, who: SimId) -> Option<&Match> {
        let tables = &self.world.script_state().pazaak;
        tables.iter().find(|table| table.player == who.0)
    }

    /// Each of `seats` asks to watch the match `player` plays (none: to stop
    /// watching), in one step.
    fn watch(&mut self, seats: &[u8], player: Option<SimId>) {
        self.step_with(asked_to_watch(seats, player));
    }

    /// Seat `seat` makes `played` at its table, in one step.
    fn play(&mut self, seat: u8, played: Move) {
        let mut input = TickInput::default();
        input.act(Seat(seat), Action::Pazaak(played));
        self.step_with(input);
    }

    /// Seat `seat` makes the move `Match::advice` has for its table.
    fn advise(&mut self, seat: u8) {
        if let Some(played) = self.last.pazaak_of(Seat(seat)).and_then(Match::advice) {
            self.play(seat, played);
        }
    }

    /// Seat `seat` leaves its table (Quit, else Forfeit), and the match's
    /// end has shown.
    fn leave_table(&mut self, seat: u8) {
        self.play(seat, Move::Quit);
        self.play(seat, Move::Forfeit);
        self.idle(steps(crate::pazaak::END_MS) + 1);
    }

    /// The host's dealer line deals it a match, which it quits; the guest's
    /// companion joins that conversation and takes "Again." (a joined player
    /// answers once the replies are up): its own match is dealt, the host
    /// still talking (R37).
    fn guest_deals(&mut self) -> Result<(), String> {
        let host = self.body(0)?;
        self.put(host, HOST_ASIDE);
        self.click(0, DEAL_FROM, 180.0)?;
        self.play(0, Move::Quit);
        self.idle(steps(crate::pazaak::END_MS) + 1);
        self.click(1, DEAL_BESIDE, 90.0)?;
        self.idle(32);
        self.answers(&[(0, Pick::Skip)]);
        self.answers(&[(1, Pick::Reply(AGAIN))]);
        Ok(())
    }

    /// Seat `seat` plays its table out as `Match::advice` says, the
    /// others waiting, until `who`'s match is over (a while at most).
    fn play_out(&mut self, seat: u8, who: SimId) {
        for _ in 0..40_000 {
            if self.table(who).is_none() {
                return;
            }
            match self.last.pazaak_of(Seat(seat)).and_then(Match::advice) {
                Some(played) => self.play(seat, played),
                None => self.step(&[]),
            }
        }
    }

    /// The run as a save of it now loads (`Save`, through its bytes).
    fn reloaded(&self) -> Result<Run, String> {
        let level = self.world.level();
        let save = crate::Save {
            hz: crate::TICK_HZ,
            level: level.id(),
            snapshot: self.last.clone(),
            script: self.world.script_state().clone(),
        };
        let save = crate::Save::decode(&save.encode()).map_err(|why| why.to_string())?;
        Ok(Run {
            world: save.world(level, self.seats, None)?,
            tick: self.tick,
            entry: self.entry,
            seats: self.seats,
            replies: self.replies.clone(),
            silent: self.silent.clone(),
            last: save.snapshot,
            notices: Vec::new(),
            reason: StepReason::Authority,
        })
    }

    /// The run as a client predicts it from its last snapshot: no scripts'
    /// state, each step a prediction.
    fn predicted(&self) -> Result<Run, String> {
        let world = super::super::SimWorld::from_snapshot(&self.last, self.world.level().clone())
            .map_err(|error| error.to_string())?;
        Ok(Run {
            world,
            tick: self.tick,
            entry: self.entry,
            seats: self.seats,
            replies: self.replies.clone(),
            silent: self.silent.clone(),
            last: self.last.clone(),
            notices: Vec::new(),
            reason: StepReason::Predict,
        })
    }

    /// `who`'s match holds a wager of `wager` out of the party's credits,
    /// as a dealer asking one would leave it.
    fn stake(&mut self, who: SimId, wager: i32) {
        let mut state = self.world.script_state().clone();
        if let Some(table) = state.pazaak.iter_mut().find(|table| table.player == who.0) {
            table.wager += wager;
            state.gold -= wager;
        }
        self.world.set_script_state(state);
    }

    /// Seat `seat` sends no command for a step: its player dropped.
    fn drop_seat(&mut self, seat: u8) {
        self.silent = vec![seat];
        self.step(&[]);
        self.silent.clear();
    }

    /// Seat `seat` back, driving placement `index` again.
    fn rejoin(&mut self, seat: u8, index: u32) -> Result<(), String> {
        self.follow(&[(index, Some(Seat(seat)))], Some(Seat(0)))?;
        self.step(&[]);
        Ok(())
    }

    /// Where each seat's body stands (metres), by id.
    fn positions(&self) -> BTreeMap<SimId, [f32; 3]> {
        (0..self.seats)
            .filter_map(|seat| self.last.body(Seat(seat)))
            .map(|entity| (entity.id, entity.position))
            .collect()
    }

    /// Every seat runs toward `yaw` for `steps` steps; how far each body
    /// went (metres), by id.
    fn run_toward(&mut self, yaw: f32, steps: usize) -> BTreeMap<SimId, f32> {
        let before = self.positions();
        for _ in 0..steps {
            let mut commands = Vec::new();
            for seat in 0..self.seats {
                if let Some(who) = self.last.body(Seat(seat)).map(|entity| entity.id) {
                    let angles = self.heading(who, yaw);
                    let forward = Command {
                        angles,
                        forward: 127,
                        ..Command::default()
                    };
                    commands.push((seat, forward));
                }
            }
            self.step(&commands);
        }
        let after = self.positions();
        before
            .iter()
            .filter_map(|(id, at)| Some((*id, apart(*at, *after.get(id)?))))
            .collect()
    }

    /// The conversation under way, if any.
    fn talk(&self) -> Option<&Conversation> {
        self.last.talk.conversations.first()
    }

    /// Conversation `id`, if it is under way.
    fn conversation(&self, id: u32) -> Option<Conversation> {
        self.last
            .talk
            .conversations
            .iter()
            .find(|c| c.id == id)
            .cloned()
    }

    /// Each of `seats` uses the merchant, whose conversation opens the store
    /// for it (P8), then every conversation ends: it trades, standing in
    /// none.
    fn shop(&mut self, seats: &[u8]) -> Result<(), String> {
        for &seat in seats {
            let (from, yaw) = if seat == 0 {
                (SHOP_BESIDE, 180.0)
            } else {
                (SHOP_FROM, 90.0)
            };
            self.click(seat, from, yaw)?;
            self.goodbye();
        }
        Ok(())
    }

    /// Whether `who` trades with the store.
    fn trading(&self, who: SimId) -> bool {
        let store = object(&self.last, STORE);
        self.last
            .goods
            .trade(who.0)
            .is_some_and(|trade| store.is_ok_and(|store| trade.store == store.0))
    }

    /// The store stocked with `cells` more cells, the party's credits
    /// `gold`: the cells' ids.
    fn stock(&mut self, cells: usize, gold: i32) -> Result<Vec<u32>, String> {
        let store = object(&self.last, STORE)?;
        let mut state = self.world.script_state().clone();
        state.gold = gold;
        let catalog = catalog();
        let made = (0..cells)
            .map(|_| state.make_item(&catalog, CELL, store.0, 1, None))
            .map(|cell| cell.map(|id| id.0).ok_or("no cell made"))
            .collect::<Result<Vec<u32>, _>>()?;
        self.world.set_script_state(state);
        Ok(made)
    }

    /// Each seat buys its cell from the store, in one step: the notices
    /// each seat was told in it.
    fn buy(&mut self, buys: &[(u8, u32)]) -> Result<BTreeMap<u8, Vec<Notice>>, String> {
        let store = object(&self.last, STORE)?.0;
        let mut input = TickInput::default();
        for &(seat, item) in buys {
            input.act(Seat(seat), Action::Buy { store, item });
        }
        self.step_with(input);
        let tick = self.tick;
        let mut told: BTreeMap<u8, Vec<Notice>> = BTreeMap::new();
        for (_, seat, notice) in self.notices.iter().filter(|(at, ..)| *at == tick) {
            told.entry(seat.0).or_default().push(notice.clone());
        }
        Ok(told)
    }

    /// Who holds `item` now.
    fn holder(&self, item: u32) -> Option<u32> {
        self.world
            .script_state()
            .items
            .get(&item)
            .map(|item| item.holder)
    }

    /// Seat 0's body walks into the proving ground's way out, a trigger to
    /// `elsewhere`: it proposes the move (P16).
    fn propose(&mut self) -> Result<(), String> {
        let ground = crate::determinism::proving_ground();
        let way_out = ground
            .placed()
            .iter()
            .find(|facts| facts.tag == "way_out")
            .map(|facts| facts.position)
            .ok_or("the ground has no way out")?;
        let host = self.body(0)?;
        self.put(
            host,
            [way_out[0] - self.entry[0], way_out[1] - self.entry[1]],
        );
        self.step(&[]);
        Ok(())
    }

    /// Seats `seats` ready for the exit proposed, in one step: the seats
    /// ready after it (a bit each), none when the party went.
    fn ready(&mut self, seats: &[u8]) -> Option<u64> {
        let mut input = TickInput::default();
        for &seat in seats {
            input.act(Seat(seat), Action::Ready);
        }
        self.step_with(input);
        self.last.ready.as_ref().map(|check| check.ready)
    }
}

/// How far apart two places are (metres).
fn apart(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = crate::math::sub(a, b);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// The least of `distances`, and the most.
fn least_most(distances: &BTreeMap<SimId, f32>) -> (f32, f32) {
    distances
        .values()
        .fold((f32::MAX, 0.0f32), |(least, most), d| {
            (least.min(*d), most.max(*d))
        })
}

/// A placement of the probes: a creature tagged and named `tag` at `at`
/// (metres) facing `yaw`, speaking `conversation` (empty: none), with no
/// scripts of its own.
fn creature(tag: &str, at: [f32; 3], yaw: f32, conversation: &str) -> ObjectData {
    ObjectData {
        tag: tag.into(),
        name: tag.into(),
        useable: true,
        conversation: conversation.into(),
        ..ObjectData::unknown("", at, yaw)
    }
}

/// The entity placement `index` stands as.
fn object(snapshot: &Snapshot, index: u32) -> Result<SimId, String> {
    snapshot
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Object(_, of) if of == index))
        .map(|entity| entity.id)
        .ok_or(format!("no object {index}"))
}

/// `body` as a link's condition: it leaves one int, the answer.
fn condition(body: &[Op]) -> Ncs {
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Rsadd(Ty::Int))
        .to(Op::Jsr, "body")
        .op(Op::Retn)
        .label("body");
    for op in body {
        asm.op(op.clone());
    }
    asm.op(Op::CpDownSp {
        offset: -8,
        size: 4,
    })
    .op(Op::MovSp(-4))
    .op(Op::Retn);
    asm.build()
}

/// `ops` and a return, as a program.
fn program(ops: &[Op]) -> Ncs {
    let mut asm = script_ncs::probes::Asm::default();
    for op in ops {
        asm.op(op.clone());
    }
    asm.op(Op::Retn);
    asm.build()
}

/// The probes' scripts and conversations:
/// - `probe_who` sets `WHO_TOKEN` to `GetIsPC(GetPCSpeaker())`;
/// - `probe_store` opens the store tagged `probe_store` for the PC
///   speaker;
/// - `probe_dark` adjusts the PC speaker's alignment (the story's);
/// - `probe_deal` deals pazaak (`PlayPazaak`, no wager), `probe_later`
///   deals it `LATER_S` after (`DelayCommand`);
/// - `probe_aborted` sets `ABORT_TOKEN` to 1;
/// - `probe_console` queues `probe_talk` with the first PC
///   (`ActionStartConversation`), `probe_lever` begins it with whoever
///   used it (`BeginConversation`);
/// - `probe_global` sets a global (the story's);
/// - `probe_busy` sets `BUSY_TOKEN` to `GetIsInConversation(GetFirstPC())`;
/// - `probe_pause` pauses the PC speaker's conversation
///   (`AssignCommand(GetPCSpeaker(), ActionPauseConversation())`);
/// - `probe_talk` (`probe_who`; "Again.", "Goodbye.", and "Join me."
///   running `probe_dark`; cut short, `probe_aborted`), `probe_bark` (a
///   one-liner), `probe_shop` (two replies, `probe_store`), `probe_deal`
///   (two replies, `probe_deal`), `probe_cut` (two replies, a cutscene:
///   `Skippable` 0), `probe_global` (two replies, `probe_global`),
///   `probe_oath` (two replies, and "Join me." running `probe_dark` back
///   to the line),
///   `probe_arrest` (two replies; cut short, `probe_dark`) and
///   `probe_chime` (two replies, its line said by the companion tagged
///   `buddy`), `probe_pause` (two replies, `probe_pause`),
///   `probe_deal_later` (two replies, `probe_later`).
fn library() -> crate::script::Library {
    const CATALOG: &str = r#"
// 0
void OpenStore(object oStore, object oPC, int nBonusMarkUp=0, int nBonusMarkDown=0);
// 1
object GetPCSpeaker();
// 2
object GetObjectByTag(string sTag, int nNth=0);
// 3
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 4
string IntToString(int nInteger);
// 5
int GetIsPC(object oCreature);
// 6
int BeginConversation(string sResRef="", object oObjectToDialog=OBJECT_INVALID);
// 7
void AdjustAlignment(object oSubject, int nAlignment, int nShift);
// 8
void PlayPazaak(int nOpponentPazaakDeck, string sEndScript, int nMaxWager, int bShowTutorial=FALSE, object oOpponent=OBJECT_INVALID);
// 9
object GetFirstPC();
// 10
void ActionStartConversation(object oObjectToConverse, string sDialogResRef="", int bPrivateConversation=FALSE, int nConversationType=0, int bIgnoreStartRange=FALSE);
// 11
object GetLastUsedBy();
// 12
void SetGlobalNumber(string sIdentifier, int nValue);
// 13
void StartNewModule(string sModuleName, string sWayPoint="", string sMovie1="", string sMovie2="", string sMovie3="", string sMovie4="", string sMovie5="", string sMovie6="");
// 14
int GetGlobalNumber(string sIdentifier);
// 15
int GetIsInConversation(object oObject);
// 16
void AssignCommand(object oActionSubject, action aActionToAssign);
// 17
void ActionPauseConversation();
// 18
void DelayCommand(float fSeconds, action aActionToDelay);
"#;
    let call = |routine, args| Op::Action { routine, args };
    let who = program(&[
        call(1, 0),
        call(5, 1),
        call(4, 1),
        Op::Const(Const::Int(WHO_TOKEN)),
        call(3, 2),
    ]);
    let store = program(&[
        Op::Const(Const::Int(0)),
        Op::Const(Const::Int(0)),
        call(1, 0),
        Op::Const(Const::Int(0)),
        Op::Const(Const::String(b"probe_store".to_vec())),
        call(2, 2),
        call(0, 4),
    ]);
    // `BeginConversation("probe_talk")`, as the games' dialogue hooks do.
    let begin = program(&[
        Op::Const(Const::Object(1)),
        Op::Const(Const::String(b"probe_talk".to_vec())),
        call(6, 2),
    ]);
    let dark = program(&[
        Op::Const(Const::Int(10)),
        Op::Const(Const::Int(3)),
        call(1, 0),
        call(7, 3),
    ]);
    let deal_ops = [
        Op::Const(Const::Object(1)),
        Op::Const(Const::Int(0)),
        Op::Const(Const::Int(0)),
        Op::Const(Const::String(Vec::new())),
        Op::Const(Const::Int(1)),
        call(8, 5),
    ];
    let deal = program(&deal_ops);
    // `DelayCommand(LATER_S, PlayPazaak(…))`: the deal, a while after its
    // line.
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::StoreState { bp: 0, sp: 0 }).to(Op::Jmp, "delay");
    for op in &deal_ops {
        asm.op(op.clone());
    }
    asm.op(Op::Retn)
        .label("delay")
        .op(Op::Const(Const::Float(LATER_S)))
        .op(call(18, 2))
        .op(Op::Retn);
    let later = asm.build();
    let aborted = program(&[
        Op::Const(Const::String(b"1".to_vec())),
        Op::Const(Const::Int(ABORT_TOKEN)),
        call(3, 2),
    ]);
    // `ActionStartConversation(GetFirstPC(), "probe_talk")`, out of range,
    // as Taris's sewer console's `k_ptar_startconv` does.
    let console = program(&[
        Op::Const(Const::Int(1)),
        Op::Const(Const::Int(0)),
        Op::Const(Const::Int(0)),
        Op::Const(Const::String(b"probe_talk".to_vec())),
        call(9, 0),
        call(10, 5),
    ]);
    // `BeginConversation("probe_talk", GetLastUsedBy())`.
    let lever = program(&[
        call(11, 0),
        Op::Const(Const::String(b"probe_talk".to_vec())),
        call(6, 2),
    ]);
    // `SetGlobalNumber("probe_global", 1)`.
    let global = program(&[
        Op::Const(Const::Int(1)),
        Op::Const(Const::String(b"probe_global".to_vec())),
        call(12, 2),
    ]);
    // `StartNewModule("elsewhere")`: the proving ground's way out.
    let mut leave_ops = vec![Op::Const(Const::String(Vec::new())); 7];
    leave_ops.push(Op::Const(Const::String(b"elsewhere".to_vec())));
    leave_ops.push(call(13, 8));
    let leave = program(&leave_ops);
    // `GetGlobalNumber("probe_wait") == 0`, as a link's condition.
    let unset = condition(&[
        Op::Const(Const::String(b"probe_wait".to_vec())),
        call(14, 1),
        Op::Const(Const::Int(0)),
        Op::Equal(Pair::IntInt),
    ]);
    // `SetCustomToken(BUSY_TOKEN, IntToString(GetIsInConversation(GetFirstPC())))`.
    let busy = program(&[
        call(9, 0),
        call(15, 1),
        call(4, 1),
        Op::Const(Const::Int(BUSY_TOKEN)),
        call(3, 2),
    ]);
    // `AssignCommand(GetPCSpeaker(), ActionPauseConversation())`.
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::StoreState { bp: 0, sp: 0 })
        .to(Op::Jmp, "assign")
        .op(call(17, 0))
        .op(Op::Retn)
        .label("assign")
        .op(call(1, 0))
        .op(call(16, 2))
        .op(Op::Retn);
    let pause = asm.build();
    let talk = Dlg {
        on_abort: "probe_aborted".into(),
        ..joining(talking(2, "probe_who"), "probe_dark")
    };
    // "Join me." leads back to the line, as "Again." does.
    let mut oath = joining(talking(2, ""), "probe_dark");
    oath.replies[usize::from(JOIN)].links = oath.replies[usize::from(AGAIN)].links.clone();
    let arrest = Dlg {
        on_abort: "probe_dark".into(),
        ..talking(2, "")
    };
    let cut = Dlg {
        skippable: false,
        ..talking(2, "")
    };
    let said_by = |tag: &str| {
        let mut chime = talking(2, "");
        chime.entries[0].speaker = tag.into();
        chime
    };
    let chime = said_by("buddy");
    let chime2 = said_by("buddy2");
    // "Again." offered while `probe_wait` is unset.
    let mut chime3 = said_by("buddy");
    chime3.entries[0].links[usize::from(AGAIN)].active.script = "probe_unset".into();
    let dialogues: BTreeMap<String, Dlg> = [
        ("probe_talk", talk),
        ("probe_bark", talking(0, "")),
        ("probe_shop", talking(2, "probe_store")),
        ("probe_deal", talking(2, "probe_deal")),
        ("probe_cut", cut),
        ("probe_global", talking(2, "probe_global")),
        ("probe_oath", oath),
        ("probe_arrest", arrest),
        ("probe_chime", chime),
        ("probe_chime2", chime2),
        ("probe_chime3", chime3),
        ("probe_pause", talking(2, "probe_pause")),
        ("probe_deal_later", talking(2, "probe_later")),
    ]
    .map(|(name, dlg)| (name.to_string(), dlg))
    .into();
    crate::probe::library(
        CATALOG,
        [
            ("probe_who", who),
            ("probe_store", store),
            ("probe_begin", begin),
            ("probe_dark", dark),
            ("probe_deal", deal),
            ("probe_aborted", aborted),
            ("probe_console", console),
            ("probe_lever", lever),
            ("probe_global", global),
            ("probe_leave", leave),
            ("probe_unset", unset),
            ("probe_busy", busy),
            ("probe_pause", pause),
            ("probe_later", later),
        ],
    )
    .with_dialogues(dialogues, BTreeMap::new())
    .with_pazaak(crate::pazaak::probes::decks())
    .with_items(catalog())
}

/// The probes' one item: a cell worth `CELL_COST`.
fn catalog() -> crate::inventory::Catalog {
    crate::inventory::Catalog::new([crate::inventory::ItemFacts {
        cost: CELL_COST,
        ..crate::world::containers::plain(CELL)
    }])
}

/// `dlg` with a third reply to its entry, "Join me.", that runs `script`
/// and ends it.
fn joining(mut dlg: Dlg, script: &str) -> Dlg {
    let mut join = dlg.replies[usize::from(GOODBYE)].clone();
    join.text.strings = vec![(0, b"Join me.".to_vec())];
    join.script.script = script.into();
    dlg.replies.push(join);
    let mut offered = dlg.entries[0].links[usize::from(GOODBYE)].clone();
    offered.index = u32::from(JOIN);
    dlg.entries[0].links.push(offered);
    dlg
}

/// Whether the transcript holds a line `wanted` picks.
fn said(run: &Run, wanted: impl Fn(&Line) -> bool) -> bool {
    run.world.script_report().talk.iter().any(wanted)
}

/// How many starts or leaves were refused for `why` (`Line::Refused`), all
/// told.
fn refusals(run: &Run, why: &str) -> usize {
    run.world
        .script_report()
        .talk
        .iter()
        .filter(|line| matches!(line, Line::Refused { why: of, .. } if *of == why))
        .count()
}

/// Each of `seats` asks to watch the match `player` plays (none: to stop),
/// as a step's actions (R9a).
fn asked_to_watch(seats: &[u8], player: Option<SimId>) -> TickInput {
    let player = player.map_or(u32::MAX, |player| player.0);
    let mut input = TickInput::default();
    for &seat in seats {
        input.act(Seat(seat), Action::Watch { player });
    }
    input
}

/// The host's answer to the request to join conversation `talk` (R5), as
/// a step's actions.
fn requested(talk: u32, accept: bool) -> TickInput {
    let mut input = TickInput::default();
    input.act(Seat(0), Action::Request { talk, accept });
    input
}

/// How many markers seat `seat` set (`Line::Marked`), all told.
fn marked_lines(run: &Run, seat: u8) -> usize {
    run.world
        .script_report()
        .talk
        .iter()
        .filter(|line| matches!(line, Line::Marked { seat: by, .. } if *by == seat))
        .count()
}

/// P6, P9: a script begins a party conversation, which every seat answers
/// (R1).
/// - Pointing marks its replies, the host's too, a watching seat's after
///   it joins mid-line holding an old byte; the marks are in the snapshot.
/// - A guest's skip ends the line; a guest's or a watching seat's answer
///   at the story's reply marks it; a watching seat's answer marks.
/// - The first answer in a step takes the line; one sent for the line
///   before, arriving while the next plays, takes nothing.
/// - The guest's answers take the replies anyone takes, and end it.
fn marks(run: &mut Run) -> Result<Vec<Case>, String> {
    let buddy = run.body(1)?;
    run.cutscene()?;
    let started = run
        .talk()
        .cloned()
        .ok_or("the script began no conversation")?;
    // Seat 2 joins now, holding a byte from before.
    run.silent.clear();
    run.replies[2] = answer(0, Pick::Point(0));
    run.step(&[]);
    let joined = run.talk().cloned();
    run.answers(&[(1, Pick::Point(GOODBYE))]);
    let marked = run.talk().cloned();
    // A guest's skip, once a skip could end the line.
    run.idle(32);
    run.answers(&[(1, Pick::Skip)]);
    let skipped = run.talk().cloned();
    run.answers(&[(2, Pick::Point(AGAIN))]);
    let both = run.talk().cloned();
    run.answers(&[(1, Pick::Point(5))]);
    let past = run.talk().cloned();
    let told = run.last.talk.clone();
    let decoded = Snapshot::decode(&run.last.encode()).map(|snapshot| snapshot.talk);
    // A mark by a seat past the last, or at a reply not offered, reads as
    // no conversation's.
    let refused = [(crate::MAX_SEATS, 0u8), (1, 99)].map(|(seat, nth)| {
        let mut bad = told.clone();
        if let Some(c) = bad.conversations.first_mut() {
            c.marks.insert(seat, nth);
        }
        let mut out = crate::wire::Writer::default();
        bad.encode(&mut out);
        crate::script::dialogue::Told::decode(&mut crate::wire::Reader::new("told", &out.0))
            .is_err()
    });
    // Seat 2 drops for a step: its mark goes; back, its held byte marks
    // nothing.
    run.silent = vec![2];
    run.step(&[]);
    let dropped = run.talk().cloned();
    run.silent.clear();
    run.step(&[]);
    let back = run.talk().cloned();
    // The story's reply: the guest's answer and the watching seat's mark
    // it; the host points too.
    run.answers(&[
        (0, Pick::Point(AGAIN)),
        (1, Pick::Reply(JOIN)),
        (2, Pick::Reply(JOIN)),
    ]);
    let story = run.talk().cloned();
    // In one step the host answers "Again." and the guest "Goodbye.": the
    // host's, the first, takes the line; the guest's mark of it goes.
    run.answers(&[(0, Pick::Reply(AGAIN)), (1, Pick::Reply(GOODBYE))]);
    let first = run.talk().cloned();
    run.idle(3);
    let repeated = run.talk().cloned();
    let lines = marked_lines(run, 1);
    // The guest's "Goodbye." sent for that line, arriving as the next plays.
    run.answers(&[(1, Pick::Reply(GOODBYE))]);
    let late = run.talk().cloned();
    run.idle(32);
    run.answers(&[(0, Pick::Skip)]);
    run.answers(&[(1, Pick::Reply(AGAIN))]);
    let guest = run.talk().cloned();
    run.idle(32);
    run.answers(&[(0, Pick::Skip)]);
    run.answers(&[(1, Pick::Reply(GOODBYE))]);
    let ended = run.last.talk.conversations.is_empty();
    // No conversation, no marks.
    let marks =
        |c: &Option<Conversation>| c.as_ref().map_or_else(BTreeMap::new, |c| c.marks.clone());
    let same = |c: &Option<Conversation>, as_: &Conversation| {
        c.as_ref()
            .is_some_and(|c| (c.id, c.shown_ms) == (as_.id, as_.shown_ms))
    };
    let waiting = |c: &Option<Conversation>| c.as_ref().is_some_and(|c| c.until_ms == u64::MAX);
    let one = BTreeMap::from([(1, GOODBYE)]);
    let two = BTreeMap::from([(1, GOODBYE), (2, AGAIN)]);
    let all = BTreeMap::from([(0, AGAIN), (1, JOIN), (2, JOIN)]);
    let after = first.as_ref().ok_or("the host's answer ended it")?;
    Ok(vec![
        (
            "first_byte_starts_count",
            check(
                same(&joined, &started) && marks(&joined).is_empty(),
                format!("marks {:?}", marks(&joined)),
            ),
        ),
        (
            "guest_points",
            check(
                same(&marked, &started) && marks(&marked) == one,
                format!("{marked:?}"),
            ),
        ),
        (
            "guest_skip_ends_line",
            check(
                same(&skipped, &started) && waiting(&skipped) && marks(&skipped) == one,
                format!("{skipped:?}"),
            ),
        ),
        (
            "watching_seat_points",
            check(
                run.last.body(Seat(2)).is_none() && marks(&both) == two,
                format!("marks {:?}", marks(&both)),
            ),
        ),
        (
            "mark_out_of_range",
            check(marks(&past) == two, format!("marks {:?}", marks(&past))),
        ),
        (
            "dropped_seat_marks_gone",
            check(
                marks(&dropped) == one && marks(&back) == one,
                format!("dropped {:?}, back {:?}", marks(&dropped), marks(&back)),
            ),
        ),
        (
            "bad_marks_refused",
            check(
                refused == [true; 2],
                format!("a seat past the last, a reply not offered: refused {refused:?}"),
            ),
        ),
        (
            "marks_in_snapshot",
            check(
                decoded.as_ref().is_ok_and(|talk| *talk == told)
                    && told.conversations.first().is_some_and(|c| c.marks == two),
                format!("{decoded:?}"),
            ),
        ),
        (
            "story_reply_only_marked",
            check(
                same(&story, &started) && marks(&story) == all,
                format!("{story:?}"),
            ),
        ),
        (
            "first_answer_wins",
            check(
                after.id == started.id
                    && after.shown_ms > started.shown_ms
                    && after.asker.0 == run.body(0)?.0
                    && after.marks.is_empty(),
                format!("{first:?}"),
            ),
        ),
        (
            "repeated_byte_marks_once",
            check(
                marks(&repeated).is_empty() && lines == 2,
                format!("{lines} marks set by seat 1"),
            ),
        ),
        (
            "late_answer_takes_nothing",
            check(
                same(&late, after) && marks(&late).is_empty(),
                format!("{late:?}"),
            ),
        ),
        (
            "guest_answer_advances",
            check(
                guest.as_ref().is_some_and(|c| {
                    c.id == started.id && c.shown_ms > after.shown_ms && c.asker.0 == buddy.0
                }) && ended,
                format!("{guest:?}, ended {ended}"),
            ),
        ),
    ])
}

/// P8: the guest's companion uses the one who talks.
/// - The conversation's player is the host's character, so
///   `GetPCSpeaker` is a PC, and the companion is the one who asked.
/// - The guest's answer ends it.
fn guest_click(run: &mut Run) -> Result<Vec<Case>, String> {
    run.forget_tokens();
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    run.put(host, HOST_ASIDE);
    run.click(1, TALK_FROM, 0.0)?;
    let talk = run.talk().cloned();
    let token = run.last.talk.tokens.get(&WHO_TOKEN).cloned();
    run.answers(&[(1, Pick::Reply(GOODBYE))]);
    let ended = run.last.talk.conversations.is_empty();
    Ok(vec![
        (
            "guest_click_pc_is_host",
            check(
                talk.as_ref()
                    .is_some_and(|c| c.pc.0 == host.0 && c.asker.0 == buddy.0)
                    && ended,
                format!("{talk:?}, ended {ended}"),
            ),
        ),
        (
            "pc_speaker_is_host",
            check(
                token.as_deref() == Some("1"),
                format!("GetIsPC(GetPCSpeaker()) {token:?}"),
            ),
        ),
    ])
}

/// Where `id`'s box stands in the last snapshot (units).
fn origin(run: &Run, id: SimId) -> Option<[f32; 3]> {
    run.last.get(id).and_then(|e| e.player).map(|p| p.origin)
}

/// How far apart two bodies' boxes stand in the last snapshot (metres).
fn between(run: &Run, a: SimId, b: SimId) -> f32 {
    match (origin(run, a), origin(run, b)) {
        (Some(a), Some(b)) => apart(a, b) * UNIT,
        _ => f32::MAX,
    }
}

/// Whether `came` stands in `centre`'s sight, and clear of every other
/// body (0.9 m).
fn in_sight_and_apart(run: &Run, centre: SimId, came: SimId) -> (bool, bool) {
    let level = run.world.level();
    let states = level.states_in(&run.last);
    let seen = matches!((origin(run, centre), origin(run, came)), (Some(c), Some(b)) if level.sees(states.as_deref(), c, b));
    let spaced = origin(run, came).is_some_and(|at| {
        run.last
            .entities
            .iter()
            .filter(|entity| entity.id != came)
            .filter_map(|entity| entity.player)
            .all(|other| {
                let d = crate::math::sub(other.origin, at);
                (d[0] * d[0] + d[1] * d[1]).sqrt() * UNIT >= 0.9
            })
    });
    (seen, spaced)
}

/// The seats a store or pazaak table was shown to in the last step.
fn shown_to(run: &Run, screen: impl Fn(&Notice) -> bool) -> Vec<Seat> {
    run.notices
        .iter()
        .filter(|(at, _, notice)| *at == run.tick && screen(notice))
        .map(|(_, seat, _)| *seat)
        .collect()
}

/// P8, R3: the guest's companion uses the merchant. Its conversation opens
/// the store for the PC speaker; it opens for whoever asked, on that seat's
/// screen alone: the companion that used it, then, the host having walked
/// up and joined, whoever's answer showed the line that opens it again, the
/// host's and then the guest's.
fn store(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    let store = object(&run.last, STORE)?;
    let opened = |notice: &Notice| matches!(notice, Notice::Show(Screen::Store { store: of, .. }) if *of == store.0);
    run.put(host, HOST_ASIDE);
    run.click(1, SHOP_FROM, 90.0)?;
    let used = (shown_to(run, opened), run.trading(buddy), run.trading(host));
    run.click(0, SHOP_BESIDE, 180.0)?;
    let mut picked = Vec::new();
    for seat in [0, 1] {
        run.idle(32);
        run.answers(&[(0, Pick::Skip)]);
        run.answers(&[(seat, Pick::Reply(AGAIN))]);
        picked.push(shown_to(run, opened));
    }
    let both = (run.trading(buddy), run.trading(host));
    run.idle(32);
    run.answers(&[(0, Pick::Skip)]);
    run.answers(&[(0, Pick::Reply(GOODBYE))]);
    Ok(vec![
        (
            "store_opens_for_asker",
            check(
                used == (vec![Seat(1)], true, false),
                format!(
                    "shown to {:?}, trading: guest {}, host {}",
                    used.0, used.1, used.2
                ),
            ),
        ),
        (
            "store_opens_for_picker",
            check(
                picked == [vec![Seat(0)], vec![Seat(1)]] && both == (true, true),
                format!("shown to {picked:?}, trading: guest and host {both:?}"),
            ),
        ),
    ])
}

/// P8: a store opens for the asker of the conversation whose script runs,
/// not of the first one the PC speaks in. The host's character is the PC of
/// two conversations: the talker's, the guest's companion standing in it (a
/// private one), and the merchant's, which the host started itself. The
/// merchant's script opens the store, on the host's screen alone and not the
/// companion's.
fn store_for_running_conversation(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    let store = object(&run.last, STORE)?;
    let (talker, merchant) = (object(&run.last, TALKER)?, object(&run.last, MERCHANT)?);
    let id = |who: SimId| ObjectId(who.0);
    run.put(host, HOST_ASIDE);
    let mut state = run.world.script_state().clone();
    let talk = &mut state.talk;
    talk.conversations.push(Conversation {
        id: talk.next,
        dialog: "probe_talk".into(),
        owner: id(talker),
        pc: id(host),
        asker: id(buddy),
        speaker: id(talker),
        replies: vec![u32::from(AGAIN), u32::from(GOODBYE)],
        until_ms: u64::MAX,
        paused: true,
        listener: id(buddy),
        ..Conversation::default()
    });
    talk.next += 1;
    talk.asked.push(Start {
        owner: id(merchant),
        pc: id(host),
        asker: id(host),
        dialog: "probe_shop".into(),
        bark: false,
    });
    run.world.set_script_state(state);
    run.step(&[]);
    let opened = |notice: &Notice| matches!(notice, Notice::Show(Screen::Store { store: of, .. }) if *of == store.0);
    let shown = shown_to(run, opened);
    let for_host = run.trading(host);
    let held = run.last.talk.conversations.len();
    // The companion's conversation is dropped, the host's own ended.
    let mut state = run.world.script_state().clone();
    state
        .talk
        .conversations
        .retain(|c| c.dialog == "probe_shop");
    run.world.set_script_state(state);
    run.idle(32);
    run.answers(&[(0, Pick::Skip)]);
    run.answers(&[(0, Pick::Reply(GOODBYE))]);
    Ok(vec![(
        "store_for_running_conversation",
        check(
            held == 2 && shown == [Seat(0)] && for_host,
            format!("{held} conversations, shown to {shown:?}, host trading {for_host}"),
        ),
    )])
}

/// R8: the guest's companion and the host each trade at the merchant's
/// store, every seat playing a body.
/// - Both buy the last cell in one step: the host's buy, the lower seat's,
///   takes it, and the guest is told the store does not sell it. With the
///   credits for one of two cells, the host's buy spends them, and the
///   guest is told it cannot afford its own.
/// - The companion is hurt: its store closes, as its Leave would close it;
///   the host's stays open.
/// - A cutscene gathers everyone: every store closes.
/// - The guest drops mid-trade: its trade ends (R12), and is not there
///   when it comes back.
fn shops(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    run.put(host, HOST_ASIDE);
    run.shop(&[1, 0])?;
    let cell = run.stock(1, 100)?;
    let last = run.buy(&[(1, cell[0]), (0, cell[0])])?;
    let last = (run.holder(cell[0]), last);
    let cells = run.stock(2, CELL_COST as i32)?;
    let credits = run.buy(&[(1, cells[1]), (0, cells[0])])?;
    let credits = ([cells[0], cells[1]].map(|cell| run.holder(cell)), credits);
    run.hurt(buddy);
    let hurt = (run.trading(host), run.trading(buddy));
    run.shop(&[1])?;
    let before = (run.trading(host), run.trading(buddy));
    run.cutscene()?;
    let gathered = (run.trading(host), run.trading(buddy));
    run.goodbye();
    run.shop(&[1])?;
    let shopping = run.trading(buddy);
    run.drop_seat(1);
    let dropped = run.trading(buddy);
    run.rejoin(1, BUDDY)?;
    let dropped = (shopping, dropped, run.trading(buddy));
    let store = object(&run.last, STORE)?.0;
    let party = Some(crate::inventory::PARTY);
    let told = |notice: Notice| BTreeMap::from([(1, vec![notice])]);
    Ok(vec![
        (
            "two_trade_same_store_last_item",
            check(
                last == (party, told(Notice::NotForSale))
                    && credits == ([party, Some(store)], told(Notice::CannotAfford)),
                format!(
                    "the last cell's holder and told: {last:?}; two cells, credits for one: {credits:?}"
                ),
            ),
        ),
        (
            "store_closes_on_damage",
            check(
                hurt == (true, false),
                format!("trading after the companion's hurt: host and guest {hurt:?}"),
            ),
        ),
        (
            "store_closes_on_cutscene",
            check(
                before == (true, true) && gathered == (false, false),
                format!("trading: host and guest {before:?}, in a cutscene {gathered:?}"),
            ),
        ),
        (
            "store_ends_on_drop",
            check(
                dropped == (true, false, false),
                format!("the guest trading, dropped, back: {dropped:?}"),
            ),
        ),
    ])
}

/// P8, P20: the guest's companion uses the dealer, whose line deals a
/// match. It is the guest's alone: its seat's screen shows it, the host's
/// moves are not its, and the guest's quit ends it.
fn pazaak(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    run.put(host, HOST_ASIDE);
    run.click(1, DEAL_FROM, 180.0)?;
    let shown = shown_to(run, |notice| *notice == Notice::Show(Screen::Pazaak));
    let player = run.last.pazaak.first().map(|table| table.player);
    let quit = Action::Pazaak(crate::pazaak::Move::Quit);
    let mut input = Vec::new();
    for seat in [0, 1] {
        run.idle(1);
        let mut acted = TickInput::default();
        acted.act(Seat(seat), quit.clone());
        run.step_with(acted);
        input.push(!run.last.pazaak.is_empty());
    }
    run.goodbye();
    Ok(vec![(
        "pazaak_for_asker",
        check(
            shown == [Seat(1)] && player == Some(buddy.0) && input == [true, false],
            format!("shown to {shown:?}, player {player:?}, under way after each quit {input:?}"),
        ),
    )])
}

/// P7, R1: while a party conversation is up (one the game began), every
/// seat's body stands still (the host's character and the guest's
/// companion), however its seat runs. When it ends, they run again.
fn held(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    run.put(host, TALK_FROM);
    run.put(buddy, BUDDY_AT);
    run.cutscene()?;
    let up = run.talk().is_some_and(|c| c.party);
    let held = run.run_toward(180.0, 25);
    run.goodbye();
    let freed = run.run_toward(180.0, 25);
    let ((_, most), (least, _)) = (least_most(&held), least_most(&freed));
    Ok(vec![
        (
            "all_held",
            check(
                up && held.len() == 2 && most < 0.02,
                format!("moved {held:?}"),
            ),
        ),
        (
            "released_after",
            check(freed.len() == 2 && least > 0.25, format!("moved {freed:?}")),
        ),
    ])
}

/// P7: a one-liner is a bark over its speaker, and holds nobody.
fn bark(run: &mut Run) -> Result<Vec<Case>, String> {
    let barker = object(&run.last, BARKER)?;
    run.click(0, BARK_FROM, 180.0)?;
    let told = &run.last.talk;
    let barked =
        told.barks.iter().any(|bark| bark.who.0 == barker.0) && told.conversations.is_empty();
    let moved = run.run_toward(0.0, 25);
    let (least, _) = least_most(&moved);
    Ok(vec![(
        "bark_holds_nobody",
        check(
            barked && moved.len() == 2 && least > 0.25,
            format!("barked {barked}, moved {moved:?}"),
        ),
    )])
}

/// P6, P7, R1: during a party conversation every seat's screen shows it,
/// the watching seat's too. When it ends, none does.
fn screens(run: &mut Run) -> Result<Vec<Case>, String> {
    run.cutscene()?;
    let id = run.talk().map(|c| c.id);
    let shown: Vec<Option<u32>> = (0..SEATS)
        .map(|seat| run.last.talk_for(Seat(seat)).map(|c| c.id))
        .collect();
    run.goodbye();
    let after = (0..SEATS).any(|seat| run.last.talk_for(Seat(seat)).is_some());
    Ok(vec![(
        "screens_show_it",
        check(
            id.is_some() && shown.iter().all(|of| *of == id) && !after,
            format!("{shown:?} of {id:?}, after {after}"),
        ),
    )])
}

/// I30 P8, R1: the guest's companion uses the one whose dialogue hook
/// begins its conversation: the companion asked it. Ended, a script of its
/// own begins it again: no guest asked that one, whose PC is the host's.
/// It is a party conversation: the companion, far off, is brought near the
/// host, in its sight and clear of everyone.
fn script_talk(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    run.put(host, HOST_ASIDE);
    run.click(1, HOOK_FROM, 0.0)?;
    let asked = run.talk().map(|c| (c.pc, c.asker));
    run.goodbye();
    run.put(buddy, FAR);
    run.step(&[]);
    let far = between(run, host, buddy);
    run.cutscene()?;
    let scripted = run.talk().map(|c| (c.pc, c.asker));
    let near = between(run, host, buddy);
    let (seen, spaced) = in_sight_and_apart(run, host, buddy);
    run.goodbye();
    let ended = run.last.talk.conversations.is_empty();
    let pc = ObjectId(host.0);
    Ok(vec![
        (
            "script_talk_no_asker",
            check(
                asked == Some((pc, ObjectId(buddy.0)))
                    && scripted == Some((pc, ObjectId::INVALID))
                    && ended,
                format!("the use's {asked:?}, the script's {scripted:?}, ended {ended}"),
            ),
        ),
        (
            "party_talk_gathers",
            check(
                far > 10.0 && near <= 4.0,
                format!("the companion {near:.2} m from the host as it began, from {far:.2} m"),
            ),
        ),
        (
            "gather_in_sight_and_apart",
            check(seen && spaced, format!("in sight {seen}, apart {spaced}")),
        ),
    ])
}

/// R1–R3 with both guests driving companions: seat 2 takes the second one,
/// and a third, no seat's, follows the host as KOTOR's party does.
/// - Two players' uses in one step start two conversations, each its
///   player's alone: held in it, its screen showing it; the seat not in
///   either is neither.
/// - The host's own conversation brings the companion following it from
///   far off, not the one the guest drives; the guest's own brings nobody.
/// - The host leading through that companion talks alone too, through it,
///   and the guest's conversation goes on.
/// - Both using the one who talks, or the one whose dialogue hook begins
///   its conversation, in one step start one conversation; the guest's
///   companion joins it.
/// - The guest's companion walking up to the host's joins it, and takes
///   its free replies.
/// - The one speaking in the guest's own turns once, at its line's start
///   (S2), to the guest's companion, its listener, not to the host's
///   character far off (R2); nothing turns it after.
/// - The guest's line pausing its PC speaker's conversation
///   (`AssignCommand(GetPCSpeaker(), ActionPauseConversation())`) while the
///   host talks too pauses the guest's, not the host's.
fn private(run: &mut Run) -> Result<Vec<Case>, String> {
    run.follow(&[(BUDDY2, Some(Seat(2))), (AIDE, None)], Some(Seat(0)))?;
    run.step(&[]);
    let (host, buddy, buddy2) = (run.body(0)?, run.body(1)?, run.body(2)?);
    let aide = object(&run.last, AIDE)?;
    let id = |who: SimId| ObjectId(who.0);
    run.clicks(&[(0, TALK_FROM, 0.0), (1, SHOP_FROM, 90.0)])?;
    let two = run.last.talk.conversations.clone();
    let screens: Vec<Option<u32>> = (0..SEATS)
        .map(|seat| run.last.talk_for(Seat(seat)).map(|c| c.id))
        .collect();
    let held = run.held();
    run.goodbye();
    // The host's own, with the guest's companion and the third far off;
    // then the guest's own, the third far off.
    run.idle(USE_GAP);
    run.put(buddy, FAR);
    run.put(aide, FAR_TOO);
    run.uses(&[(0, TALK_FROM, 0.0)])?;
    let alone = run.talk().cloned();
    let (driven, free) = (between(run, host, buddy), between(run, host, aide));
    run.goodbye();
    run.idle(USE_GAP);
    run.put(host, HOST_ASIDE);
    run.put(aide, FAR_TOO);
    run.uses(&[(1, TALK_FROM, 0.0)])?;
    let guests = run.talk().cloned();
    let stayed = between(run, buddy, aide);
    run.step(&[]);
    let turned = [buddy, host].map(|to| facing_off(run, TALKER, to));
    run.goodbye();
    run.put(aide, AIDE_AT);
    // The host leads through the third (`world::lent`), the guest talking
    // with the merchant meanwhile.
    run.lead(Some(AIDE))?;
    run.click(1, SHOP_FROM, 90.0)?;
    run.click(0, TALK_FROM, 0.0)?;
    let lent = run.last.talk.conversations.clone();
    // Leading through it, the host takes the story's replies itself (its
    // screen sends them as replies, not marks), in its own conversation
    // and in one the game starts.
    let takes = |run: &Run, talk: Option<&Conversation>| {
        talk.map(|talk| run.last.takes_story(Seat(0), talk))
    };
    let mut leading_takes = vec![takes(run, lent.iter().find(|c| c.listener == id(aide)))];
    run.goodbye();
    run.cutscene()?;
    leading_takes.push(takes(run, run.talk()));
    run.goodbye();
    run.lead(None)?;
    run.follow(&[(AIDE, None)], None)?;
    run.put(buddy, BUDDY_AT);
    run.put(aide, AIDE_AT);
    let mut same = Vec::new();
    for (to, beside, yaw) in [
        (TALK_FROM, TALK_BESIDE, 90.0),
        (HOOK_FROM, HOOK_BESIDE, 180.0),
    ] {
        run.clicks(&[(0, to, 0.0), (1, beside, yaw)])?;
        same.push(run.last.talk.conversations.clone());
        run.goodbye();
    }
    run.click(0, TALK_FROM, 0.0)?;
    run.click(1, TALK_BESIDE, 90.0)?;
    let walked = run.last.talk.conversations.clone();
    run.idle(32);
    run.answers(&[(1, Pick::Skip)]);
    run.answers(&[(1, Pick::Reply(AGAIN))]);
    let answered = run.talk().cloned();
    run.goodbye();
    // The guest's line pauses its PC speaker's conversation while the host
    // talks too: the guest's holds, the host's goes on.
    let hosts = run.start(0, HOOKED, "probe_talk")?;
    let paused = run.start(1, TALKER, "probe_pause")?;
    run.idle(3);
    let pauses = [paused, hosts].map(|talk| run.conversation(talk).map(|c| c.paused));
    for talk in [paused, hosts] {
        run.edit(talk, |c| c.paused = false);
    }
    run.goodbye();
    let listeners: Vec<(ObjectId, bool)> = two.iter().map(|c| (c.listener, c.party)).collect();
    let ids: Vec<Option<u32>> = two.iter().map(|c| Some(c.id)).chain([None]).collect();
    let holds = [host, buddy, buddy2].map(|who| held.get(&who).copied());
    let joined_by = |talk: &[Conversation]| {
        talk.len() == 1 && talk[0].listener == id(host) && talk[0].joined == [id(buddy)]
    };
    let lent_listeners: Vec<(ObjectId, bool)> =
        lent.iter().map(|c| (c.listener, c.party)).collect();
    Ok(vec![
        (
            "two_at_once",
            check(
                listeners == [(id(host), false), (id(buddy), false)]
                    && holds == [Some(true), Some(true), Some(false)]
                    && screens == ids,
                format!("listeners and party {listeners:?}, held {holds:?}, screens {screens:?}"),
            ),
        ),
        (
            "host_private_no_gather",
            check(
                alone.as_ref().is_some_and(|c| !c.party) && driven > 10.0 && free <= 4.0,
                format!(
                    "the guest's companion {driven:.2} m from the host, the free one {free:.2} m"
                ),
            ),
        ),
        (
            "guest_private_gathers_nobody",
            check(
                guests
                    .as_ref()
                    .is_some_and(|c| c.listener == id(buddy) && !c.party)
                    && stayed > 10.0,
                format!("{guests:?}, the host's companion {stayed:.2} m from the guest's"),
            ),
        ),
        (
            "lent_host_private",
            check(
                lent_listeners == [(id(buddy), false), (id(aide), false)],
                format!("listeners and party {lent_listeners:?}"),
            ),
        ),
        (
            "lent_host_takes_story",
            check(
                leading_takes == [Some(true); 2],
                format!("its own, a cutscene: {leading_takes:?}"),
            ),
        ),
        (
            "same_npc_same_step",
            check(joined_by(&same[0]), format!("{:?}", same[0])),
        ),
        (
            "hooked_same_step",
            check(joined_by(&same[1]), format!("{:?}", same[1])),
        ),
        (
            "walk_up_joins",
            check(
                joined_by(&walked)
                    && answered.as_ref().is_some_and(|c| {
                        c.id == walked[0].id
                            && c.shown_ms > walked[0].shown_ms
                            && c.asker == id(buddy)
                    }),
                format!("{walked:?}, after the guest's answer {answered:?}"),
            ),
        ),
        (
            "speaker_faces_listener",
            check(
                matches!(turned, [Ok(listener), Ok(host)] if listener < 2.0 && host > 10.0),
                format!("degrees off the guest's companion, the host: {turned:?}"),
            ),
        ),
        (
            "pause_holds_its_own",
            check(
                pauses == [Some(true), Some(false)],
                format!("the guest's paused, the host's: {pauses:?}"),
            ),
        ),
    ])
}

/// How far (degrees) placement `index` faces from `toward`.
fn facing_off(run: &Run, index: u32, toward: SimId) -> Result<f32, String> {
    faces_off(run, object(&run.last, index)?, toward)
}

/// How far (degrees) `who` faces from `toward`.
fn faces_off(run: &Run, who: SimId, toward: SimId) -> Result<f32, String> {
    let entity = |id: SimId| run.last.get(id).ok_or(format!("no {id:?}"));
    let (from, to) = (entity(who)?, entity(toward)?);
    let yaw = from.player.as_ref().ok_or("it does not move")?.view[1];
    let (a, b) = (from.position, to.position);
    let off = (yaw - crate::math::atan2_deg(b[1] - a[1], b[0] - a[0])).rem_euclid(360.0);
    Ok(off.min(360.0 - off))
}

/// R1, R7, R10: what holds every seat, and placeables' uses.
/// - A conversation a script begins cuts the guest's own short (its
///   `EndConverAbort` runs), and holds every seat.
/// - One whose owner is the guest's to talk with is refused, and cuts
///   nothing.
/// - A console whose `OnUsed` queues a conversation with the first PC
///   (`ActionStartConversation`, as Taris's sewer console does), used by
///   the guest's companion, starts the guest's own.
/// - A lever whose `OnUsed` begins one with `GetLastUsedBy()`: the
///   guest's own too, the scripts' PC the host's character.
/// - The guest's use of one whose conversation is a cutscene starts a
///   party conversation, locked, holding every seat.
fn cutscenes(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy, buddy2) = (run.body(0)?, run.body(1)?, run.body(2)?);
    let (console, lever) = (object(&run.last, CONSOLE)?, object(&run.last, LEVER)?);
    let cutter = object(&run.last, CUTTER)?;
    let id = |who: SimId| ObjectId(who.0);
    let everyone = |held: &BTreeMap<SimId, bool>| {
        [host, buddy, buddy2]
            .iter()
            .all(|who| held.get(who) == Some(&true))
    };
    let cut_short = |run: &Run, of: Option<u32>| {
        said(
            run,
            |line| matches!(line, Line::CutShort { id, .. } if Some(*id) == of),
        )
    };
    run.forget_tokens();
    run.put(host, HOST_ASIDE);
    run.click(1, TALK_BESIDE, 90.0)?;
    let private = run.talk().map(|c| c.id);
    run.cutscene()?;
    let token = run.last.talk.tokens.get(&ABORT_TOKEN).cloned();
    let noted = cut_short(run, private);
    let scene = run.last.talk.conversations.clone();
    let held = run.held();
    run.goodbye();
    // The one whose script begins the scene is the guest's to talk with.
    run.forget_tokens();
    run.click(1, HOOK_FROM, 0.0)?;
    let hooked = run.talk().map(|c| c.id);
    run.cutscene()?;
    let kept = run
        .last
        .talk
        .conversations
        .iter()
        .map(|c| c.id)
        .collect::<Vec<_>>();
    let spared = !cut_short(run, hooked) && !run.last.talk.tokens.contains_key(&ABORT_TOKEN);
    run.goodbye();
    let mut used = Vec::new();
    for from in [CONSOLE_FROM, LEVER_FROM] {
        run.forget_tokens();
        run.click(1, from, 0.0)?;
        // The queued conversation starts once the action is at the front.
        run.step(&[]);
        let token = run.last.talk.tokens.get(&WHO_TOKEN).cloned();
        used.push((run.talk().cloned(), token));
        run.goodbye();
    }
    run.click(1, CUT_FROM, 180.0)?;
    let clicked = run.talk().cloned();
    let cut_held = run.held();
    run.goodbye();
    let party = |talk: Option<&Conversation>| {
        talk.is_some_and(|c| c.party && c.locked && c.listener == id(host))
    };
    let guests = |(talk, token): &(Option<Conversation>, Option<String>), owner: SimId| {
        talk.as_ref().is_some_and(|c| {
            c.owner == id(owner)
                && c.pc == id(host)
                && c.listener == id(buddy)
                && c.asker == id(buddy)
                && !c.party
        }) && token.as_deref() == Some("1")
    };
    Ok(vec![
        (
            "cutscene_cuts_private",
            check(
                private.is_some()
                    && token.as_deref() == Some("1")
                    && noted
                    && scene.len() == 1
                    && party(scene.first())
                    && everyone(&held),
                format!("aborted {token:?}, noted {noted}, then {scene:?}, held {held:?}"),
            ),
        ),
        (
            "refused_cutscene_cuts_nothing",
            check(
                hooked.is_some() && kept == Vec::from_iter(hooked) && spared,
                format!("the guest's {hooked:?}, then {kept:?}, spared {spared}"),
            ),
        ),
        (
            "onused_asker",
            check(guests(&used[0], console), format!("{:?}", used[0])),
        ),
        (
            "pc_named_guest_is_host",
            check(guests(&used[1], lever), format!("{:?}", used[1])),
        ),
        (
            "cutscene_dlg_by_click",
            check(
                party(clicked.as_ref())
                    && clicked.as_ref().is_some_and(|c| c.owner == id(cutter))
                    && everyone(&cut_held),
                format!("{clicked:?}, held {cut_held:?}"),
            ),
        ),
    ])
}

/// R5, both guests driving companions: a story reply in a guest's own
/// conversation asks the host in.
/// - The guest's pick of the story's reply asks: the line stays, marked;
///   its free replies are still the guest's to take.
/// - The host accepting makes it the party's: the host's character its
///   listener, the guest joined, every seat held; the host takes the
///   story's reply.
/// - The host ending it cuts it short.
/// - While the party is in combat the host's answer does nothing; out of
///   it, the host's next answer is taken.
/// - So while the host stands in a locked conversation of its own;
///   unlocked, accepting cuts it short. The host's screen says why its
///   keys wait meanwhile (`Snapshot::request_wait`).
/// - The host walking up to one that asks answers it itself: the request
///   goes. The host ends one that asks though it stands in it, locked.
/// - Both guests asking at once: accepting one leaves the other asking,
///   where it stands, though far from the accepted one's NPC.
/// - The host's answer round-trips as bytes; a bad accept byte is refused.
fn requests(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy, buddy2) = (run.body(0)?, run.body(1)?, run.body(2)?);
    let id = |who: SimId| ObjectId(who.0);
    let good_evil = |run: &Run| {
        run.last
            .get(host)
            .and_then(|entity| entity.sheet)
            .map(|sheet| sheet.good_evil)
    };
    run.put(host, HOST_ASIDE);
    run.click(1, TALK_FROM, 0.0)?;
    let opened = run
        .talk()
        .cloned()
        .ok_or("the guest's use started no conversation")?;
    run.answers(&[(1, Pick::Reply(JOIN))]);
    let asked = run.talk().cloned();
    let icons = |run: &Run| [0, 1, 2].map(|seat| run.last.busy(Seat(seat)));
    let asking_icons = icons(run);
    let noted = said(
        run,
        |line| matches!(line, Line::Asked { id: of, by, .. } if *of == opened.id && *by == id(buddy)),
    );
    run.answers(&[(1, Pick::Reply(AGAIN))]);
    let free = run.talk().cloned();
    let before = good_evil(run);
    run.request(opened.id, true);
    let accepted = run.talk().cloned();
    let gathered_icons = icons(run);
    let held = run.held();
    run.answers(&[(0, Pick::Reply(JOIN))]);
    let taken = said(
        run,
        |line| matches!(line, Line::Picked { id: of, reply, .. } if *of == opened.id && *reply == u32::from(JOIN)),
    ) && run.last.talk.conversations.is_empty();
    let after = good_evil(run);
    // The host ends the next.
    let second = run.ask(&[(1, TALK_FROM, 0.0)])?[0];
    run.forget_tokens();
    run.request(second, false);
    let aborted = said(
        run,
        |line| matches!(line, Line::Ended { id: of, aborted: true, .. } if *of == second),
    );
    let ended = (
        run.last.talk.conversations.is_empty(),
        aborted,
        run.last.talk.tokens.get(&ABORT_TOKEN).cloned(),
    );
    // The host answers in combat, then out of it.
    let third = run.ask(&[(1, TALK_FROM, 0.0)])?[0];
    run.fight(true)?;
    run.request(third, true);
    let fighting = (run.last.in_combat, run.talk().cloned());
    let mut waits = vec![run.last.request_wait()];
    run.fight(false)?;
    run.step(&[]);
    let calm = (run.last.in_combat, run.talk().cloned());
    waits.push(run.last.request_wait());
    run.request(third, true);
    let answered = run.talk().cloned();
    run.goodbye();
    // The host in its own conversation, locked, then not.
    let fourth = run.ask(&[(1, TALK_FROM, 0.0)])?[0];
    run.click(0, HOOK_FROM, 0.0)?;
    let own = run
        .last
        .talk
        .conversations
        .iter()
        .find(|c| c.listener == id(host))
        .map_or(u32::MAX, |c| c.id);
    run.edit(own, |c| c.locked = true);
    run.request(fourth, true);
    let locked = (run.conversation(own).is_some(), run.conversation(fourth));
    waits.push(run.last.request_wait());
    run.edit(own, |c| c.locked = false);
    run.request(fourth, true);
    let cut = said(
        run,
        |line| matches!(line, Line::CutShort { id: of, .. } if *of == own),
    ) && said(
        run,
        |line| matches!(line, Line::Ended { id: of, aborted: true, .. } if *of == own),
    );
    let joined_in = run.conversation(fourth);
    run.goodbye();
    // The host walks up to a guest's that asks; then, standing in another
    // that asks and is locked, it ends that one.
    let fifth = run.ask(&[(1, TALK_FROM, 0.0)])?[0];
    run.click(0, TALK_BESIDE, 90.0)?;
    let walked_up = run.conversation(fifth);
    run.goodbye();
    let sixth = run.ask(&[(1, TALK_FROM, 0.0)])?[0];
    run.edit(sixth, |c| {
        c.locked = true;
        c.joined.push(id(host));
    });
    run.request(sixth, false);
    let ended_locked = run.conversation(sixth).is_none()
        && said(
            run,
            |line| matches!(line, Line::Ended { id: of, aborted: true, .. } if *of == sixth),
        );
    run.goodbye();
    // Both guests ask; the second then stands far from the first's NPC.
    let two = run.ask(&[(1, TALK_FROM, 0.0), (2, HOOK_FROM, 0.0)])?;
    let asked_two = run.last.talk.conversations.clone();
    run.put(buddy2, FAR_TOO);
    run.step(&[]);
    let stood = origin(run, buddy2);
    let far = between(run, buddy2, object(&run.last, TALKER)?);
    run.request(two[0], true);
    let one_accepted = run.last.talk.conversations.clone();
    let stayed = match (stood, origin(run, buddy2)) {
        (Some(was), Some(now)) => apart(was, now) * UNIT,
        _ => f32::MAX,
    };
    run.goodbye();
    let waiting = |c: &Option<Conversation>| {
        c.as_ref()
            .is_some_and(|c| c.id == third && c.asking == id(buddy) && !c.party)
    };
    let asking = |talk: &[Conversation], who: SimId| {
        talk.iter()
            .any(|c| c.listener == id(who) && c.asking == id(who) && !c.party)
    };
    let request = |accept| Action::Request { talk: 7, accept };
    let trips = [true, false]
        .map(|accept| Action::from_bytes(&request(accept).to_bytes()) == Ok(request(accept)));
    let mut bad = request(true).to_bytes();
    if let Some(last) = bad.last_mut() {
        *last = 2;
    }
    let refused = Action::from_bytes(&bad).is_err();
    let holds = [host, buddy, buddy2].map(|who| held.get(&who).copied());
    Ok(vec![
        (
            "guest_story_asks",
            check(
                asked.as_ref().is_some_and(|c| {
                    (c.id, c.shown_ms) == (opened.id, opened.shown_ms)
                        && c.asking == id(buddy)
                        && c.marks == BTreeMap::from([(1, JOIN)])
                        && !c.party
                }) && noted,
                format!("{asked:?}, noted {noted}"),
            ),
        ),
        (
            "free_still_free_while_asking",
            check(
                free.as_ref().is_some_and(|c| {
                    c.id == opened.id && c.shown_ms > opened.shown_ms && c.asking == id(buddy)
                }),
                format!("{free:?}"),
            ),
        ),
        (
            "host_accepts",
            check(
                accepted.as_ref().is_some_and(|c| {
                    c.party
                        && c.listener == id(host)
                        && c.joined == [id(buddy)]
                        && c.asking == ObjectId::INVALID
                }) && holds == [Some(true); 3]
                    && taken
                    && matches!((before, after), (Some(was), Some(now)) if now < was),
                format!(
                    "{accepted:?}, held {holds:?}, the host took it {taken}, good/evil {before:?} then {after:?}"
                ),
            ),
        ),
        (
            "host_ends",
            check(
                ended == (true, true, Some("1".into())),
                format!("gone, ended cut short, aborted token: {ended:?}"),
            ),
        ),
        (
            "request_waits_in_combat",
            check(
                fighting.0
                    && waiting(&fighting.1)
                    && !calm.0
                    && waiting(&calm.1)
                    && answered
                        .as_ref()
                        .is_some_and(|c| c.id == third && c.party && c.listener == id(host)),
                format!("in combat {fighting:?}, out of it {calm:?}, answered {answered:?}"),
            ),
        ),
        (
            "request_waits_while_host_locked",
            check(
                locked.0
                    && locked
                        .1
                        .as_ref()
                        .is_some_and(|c| c.asking == id(buddy) && !c.party)
                    && cut
                    && joined_in
                        .as_ref()
                        .is_some_and(|c| c.party && c.listener == id(host)),
                format!(
                    "locked: the host's still up, the guest's {:?}; unlocked: the host's cut short {cut}, then {joined_in:?}",
                    locked.1
                ),
            ),
        ),
        (
            "request_wait_said",
            check(
                waits
                    == [
                        Some(RequestWait::InCombat),
                        None,
                        Some(RequestWait::Talking),
                    ],
                format!("in combat, out of it, the host's own locked: {waits:?}"),
            ),
        ),
        (
            "two_asks_queue",
            check(
                asked_two.len() == 2
                    && asking(&asked_two, buddy)
                    && asking(&asked_two, buddy2)
                    && one_accepted.len() == 2
                    && one_accepted.iter().any(|c| c.id == two[0] && c.party)
                    && asking(&one_accepted, buddy2),
                format!("{asked_two:?}, then {one_accepted:?}"),
            ),
        ),
        (
            "accept_leaves_other_talks",
            check(
                far > 10.0 && stayed < 0.05 && asking(&one_accepted, buddy2),
                format!("the other guest {far:.2} m from the NPC, moved {stayed:.2} m"),
            ),
        ),
        (
            "host_joining_clears_request",
            check(
                walked_up.as_ref().is_some_and(|c| {
                    c.joined == [id(host)] && c.asking == ObjectId::INVALID && !c.party
                }),
                format!("{walked_up:?}"),
            ),
        ),
        (
            "host_ends_locked_asked",
            check(ended_locked, format!("ended cut short {ended_locked}")),
        ),
        (
            "request_round_trips",
            check(
                trips == [true; 2] && refused,
                format!("round trips {trips:?}, a bad accept byte refused {refused}"),
            ),
        ),
        (
            "busy_asking_then_talking",
            check(
                asking_icons == [None, Some(Busy::Asking), None]
                    && gathered_icons == [Some(Busy::Talking); 3],
                format!("asking {asking_icons:?}, accepted {gathered_icons:?}"),
            ),
        ),
    ])
}

/// R6, both guests driving companions: leaving, and what locks.
/// - The guest leaving its own unlocked conversation ends it cut short
///   (its `EndConverAbort` runs) and frees its companion.
/// - The guest joined to the host's leaving steps out alone: the host's
///   goes on, the host held.
/// - A line whose script may set a global locks its conversation: the
///   guest's Leave is refused.
/// - The host taking the story's reply in its own locks it: its Leave is
///   refused.
/// - One whose `EndConverAbort` may adjust alignment is locked from its
///   first line.
/// - A cutscene is locked: neither the host's Leave nor a guest's ends it.
/// - The guest leaving its own in the step the host accepts its request:
///   the accept goes first, so the guest stands in the party's and its
///   Leave is refused; the host, its listener, may leave it, leading
///   through a companion too.
/// - The Leave row shows only while more than one seat plays a body, as
///   the sim decides (`together`): not for the host whose guests watch.
fn leaving(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    let id = |who: SimId| ObjectId(who.0);
    let aborted = |run: &Run, talk: u32| {
        said(
            run,
            |line| matches!(line, Line::Ended { id, aborted: true, .. } if *id == talk),
        ) && run.last.talk.tokens.get(&ABORT_TOKEN).map(String::as_str) == Some("1")
    };
    run.put(host, HOST_ASIDE);
    run.forget_tokens();
    run.click(1, TALK_FROM, 0.0)?;
    let own = run
        .talk()
        .cloned()
        .ok_or("the guest's use started no conversation")?;
    // Which pick a faded story reply sends (R5, I30 P6): the companion's in
    // its own asks; a player's own character's there, or anyone's joined
    // to the host's, only points.
    let as_avatar = |snapshot: &Snapshot| {
        let mut snapshot = snapshot.clone();
        if let Some(entity) = snapshot.entities.iter_mut().find(|e| e.id == buddy) {
            entity.kind = Kind::Avatar(Seat(1));
        }
        snapshot
    };
    let mut asks = vec![
        run.last.asks_host(Seat(1), &own),
        as_avatar(&run.last).asks_host(Seat(1), &own),
    ];
    let (up, _) = run.leave(1, own.id, "locked");
    let left = !up && aborted(run, own.id);
    let freed = run.held().get(&buddy).copied();
    run.click(0, TALK_FROM, 0.0)?;
    run.click(1, TALK_BESIDE, 90.0)?;
    let joined = run.talk().cloned();
    if let Some(joined) = &joined {
        asks.push(run.last.asks_host(Seat(1), joined));
        asks.push(as_avatar(&run.last).asks_host(Seat(1), joined));
    }
    run.answers(&[(1, Pick::Leave)]);
    let stepped_out = run.talk().cloned();
    // The Leave row: the host's own unlocked, then one seat alone playing a
    // body though three are held (the guests watching).
    let alone = |snapshot: &Snapshot| {
        let mut snapshot = snapshot.clone();
        for entity in &mut snapshot.entities {
            if let Some(fighter) = entity.fighter.as_mut() {
                fighter.mind.driver = None;
            }
        }
        snapshot
    };
    let rows = stepped_out.as_ref().map(|c| {
        [run.last.clone(), alone(&run.last)].map(|snapshot| snapshot.may_leave(Seat(0), c))
    });
    let held = run.held();
    run.goodbye();
    let mut locks = Vec::new();
    for dialog in ["probe_global", "probe_arrest"] {
        let talk = run.start(1, TALKER, dialog)?;
        let first = run.conversation(talk);
        locks.push((first, run.leave(1, talk, "locked")));
        run.goodbye();
    }
    let oath = run.start(0, TALKER, "probe_oath")?;
    let open = run.conversation(oath);
    run.answers(&[(0, Pick::Reply(JOIN))]);
    let sworn = run.conversation(oath);
    let host_refused = run.leave(0, oath, "locked");
    run.goodbye();
    run.cutscene()?;
    let scene = run
        .talk()
        .cloned()
        .ok_or("the script began no conversation")?;
    let scene_left = [0, 1].map(|seat| run.leave(seat, scene.id, "locked"));
    run.goodbye();
    run.put(host, HOST_ASIDE);
    let asked = run.ask(&[(1, TALK_FROM, 0.0)])?[0];
    let before = refusals(run, "the host's");
    run.answers_with(&[(1, Pick::Leave)], requested(asked, true));
    let accepted = run.conversation(asked);
    let guest_refused = refusals(run, "the host's") - before;
    run.forget_tokens();
    let (host_stayed, _) = run.leave(0, asked, "locked");
    let host_left = !host_stayed && aborted(run, asked);
    run.goodbye();
    // The host leading through the third companion leaves one it accepted:
    // its screen offers the Leave, and it ends.
    run.follow(&[(AIDE, None)], Some(Seat(0)))?;
    run.lead(Some(AIDE))?;
    let lent = run.ask(&[(1, TALK_FROM, 0.0)])?[0];
    run.request(lent, true);
    let lent_row = run
        .conversation(lent)
        .map(|c| run.last.may_leave(Seat(0), &c));
    run.forget_tokens();
    let (lent_stayed, _) = run.leave(0, lent, "locked");
    let lent_left = !lent_stayed && aborted(run, lent);
    run.goodbye();
    run.lead(None)?;
    run.follow(&[(AIDE, None)], None)?;
    let unlocked = |c: &Option<Conversation>| c.as_ref().is_some_and(|c| !c.locked && !c.party);
    let locked_from_first = |(first, left): &(Option<Conversation>, (bool, usize))| {
        first.as_ref().is_some_and(|c| c.locked && !c.party) && *left == (true, 1)
    };
    Ok(vec![
        (
            "story_pick_asks_only_alone",
            check(
                asks == [true, false, false, false],
                format!(
                    "the companion's own, a player's own character there, joined to the host's as either: {asks:?}"
                ),
            ),
        ),
        (
            "leave_unlocked",
            check(
                !own.locked && left && freed == Some(false),
                format!(
                    "{own:?}, ended cut short and aborted {left}, the companion held {freed:?}"
                ),
            ),
        ),
        (
            "joined_leaves_alone",
            check(
                joined.as_ref().is_some_and(|c| c.joined == [id(buddy)])
                    && stepped_out.as_ref().is_some_and(|c| {
                        joined.as_ref().is_some_and(|was| was.id == c.id)
                            && c.listener == id(host)
                            && c.joined.is_empty()
                    })
                    && held.get(&host) == Some(&true)
                    && held.get(&buddy) == Some(&false),
                format!("{joined:?}, then {stepped_out:?}, held {held:?}"),
            ),
        ),
        (
            "locked_by_story_line",
            check(locked_from_first(&locks[0]), format!("{:?}", locks[0])),
        ),
        (
            "locked_by_reply",
            check(
                unlocked(&open)
                    && sworn.as_ref().is_some_and(|c| c.locked)
                    && host_refused == (true, 1),
                format!(
                    "{open:?}, then {sworn:?}, the host's leave (up, refused) {host_refused:?}"
                ),
            ),
        ),
        (
            "abort_script_story_locks",
            check(locked_from_first(&locks[1]), format!("{:?}", locks[1])),
        ),
        (
            "cutscene_locked",
            check(
                scene.locked && scene.party && scene_left == [(true, 1); 2],
                format!("{scene:?}, leaves (up, refused) {scene_left:?}"),
            ),
        ),
        (
            "accept_same_step_as_leave",
            check(
                accepted.as_ref().is_some_and(|c| {
                    c.party && c.listener == id(host) && c.joined == [id(buddy)] && !c.locked
                }) && guest_refused == 1,
                format!("{accepted:?}, the guest's leave refused {guest_refused}"),
            ),
        ),
        (
            "host_leaves_accepted",
            check(
                host_left,
                format!("ended cut short and aborted {host_left}"),
            ),
        ),
        (
            "lent_host_leaves_accepted",
            check(
                lent_row == Some(Some(true)) && lent_left,
                format!("the Leave row {lent_row:?}, ended cut short and aborted {lent_left}"),
            ),
        ),
        (
            "leave_row_needs_two_bodies",
            check(
                rows == Some([Some(true), None]),
                format!("the host's own, with the guests playing, then watching: {rows:?}"),
            ),
        ),
    ])
}

/// Whether conversation `talk` was cut short for `why` and ended so
/// (`EndConverAbort`).
fn cut_for(run: &Run, talk: Option<u32>, why: &str) -> bool {
    said(
        run,
        |line| matches!(line, Line::CutShort { id, why: of, .. } if Some(*id) == talk && *of == why),
    ) && said(
        run,
        |line| matches!(line, Line::Ended { id, aborted: true, .. } if Some(*id) == talk),
    )
}

/// R4, both guests driving companions: the host's conversation reaches a
/// line its companion says, the one the guest drives.
/// - The guest in its own unlocked conversation: that one is cut short
///   (its `EndConverAbort` runs) and the companion joins the host's where
///   it stands, never moved; the line plays.
/// - The guest in its own locked one: the line waits on it; once the
///   guest's ends, it plays, the companion joined.
/// - The guest's locked one outlasting `INTERJECTION_WAIT_MS`: the line
///   plays without the companion, the guest's goes on.
/// - The line said by the second guest's companion, joined to the first
///   guest's unlocked conversation: it steps out of it (the first guest's
///   goes on) and joins the host's; joined to a locked one, the line
///   waits on it.
/// - A waited line offers its replies as their conditions stand when it
///   plays, not as they stood when it began to wait.
fn interjections(run: &mut Run) -> Result<Vec<Case>, String> {
    use crate::script::dialogue::INTERJECTION_WAIT_MS;
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    let id = |who: SimId| ObjectId(who.0);
    let chimed = |run: &Run, talk: u32| {
        said(
            run,
            |line| matches!(line, Line::Entry { id: of, speaker, .. } if *of == talk && *speaker == id(buddy)),
        )
    };
    let joined = |c: &Option<Conversation>, by: &[ObjectId]| {
        c.as_ref()
            .is_some_and(|c| c.joined == by && c.waits_for == ObjectId::INVALID)
    };
    run.put(host, HOST_ASIDE);
    run.forget_tokens();
    run.click(1, TALK_FROM, 0.0)?;
    let own = run.talk().map(|c| c.id);
    let stood = origin(run, buddy);
    let pulled = run.start(0, BARKER, "probe_chime")?;
    let pulled_in = run.conversation(pulled);
    let aborted = run.last.talk.tokens.get(&ABORT_TOKEN).cloned();
    let cut = cut_for(run, own, "an interjection") && aborted.as_deref() == Some("1");
    let moved = match (stood, origin(run, buddy)) {
        (Some(was), Some(now)) => apart(was, now) * UNIT,
        _ => f32::MAX,
    };
    let pulled_chimed = chimed(run, pulled);
    run.goodbye();
    // The guest's own locked: the line waits, then plays once it ends.
    let locked = run.start(1, TALKER, "probe_talk")?;
    run.edit(locked, |c| c.locked = true);
    let waits = run.start(0, BARKER, "probe_chime")?;
    let waiting = (run.conversation(waits), chimed(run, waits));
    run.answers(&[(1, Pick::Reply(GOODBYE))]);
    let played = (run.conversation(waits), chimed(run, waits));
    run.goodbye();
    // The guest's locked one outlasts the wait.
    let held = run.start(1, TALKER, "probe_talk")?;
    run.edit(held, |c| c.locked = true);
    let capped = run.start(0, BARKER, "probe_chime")?;
    run.idle(steps(INTERJECTION_WAIT_MS) - 2);
    let before_cap = chimed(run, capped);
    run.idle(4);
    let after_cap = (run.conversation(capped), chimed(run, capped));
    let guest_on = run.conversation(held).is_some();
    run.goodbye();
    // The second guest's companion joined to the first guest's.
    let buddy2 = run.body(2)?;
    let joined_to_guest = |run: &mut Run, locked: bool| -> Result<(u32, u32), String> {
        let guests = run.start(1, TALKER, "probe_talk")?;
        run.edit(guests, |c| {
            c.joined.push(id(buddy2));
            c.locked = locked;
        });
        Ok((guests, run.start(0, BARKER, "probe_chime2")?))
    };
    let (guests, hosts) = joined_to_guest(run, false)?;
    let stepped = (run.conversation(guests), run.conversation(hosts));
    run.goodbye();
    let (guests, hosts) = joined_to_guest(run, true)?;
    let on_hold = (run.conversation(guests), run.conversation(hosts));
    run.goodbye();
    // A waited line's replies, offered again as it plays.
    let locked = run.start(1, TALKER, "probe_talk")?;
    run.edit(locked, |c| c.locked = true);
    let anew = run.start(0, BARKER, "probe_chime3")?;
    let offered_first = run.conversation(anew).map(|c| c.replies);
    let mut state = run.world.script_state().clone();
    state.numbers.insert("probe_wait".into(), 1);
    run.world.set_script_state(state);
    run.answers(&[(1, Pick::Reply(GOODBYE))]);
    let offered_then = run.conversation(anew).map(|c| c.replies);
    let mut state = run.world.script_state().clone();
    state.numbers.remove("probe_wait");
    run.world.set_script_state(state);
    // Its only reply now, "Goodbye.", ends it.
    run.answers(&[(0, Pick::Reply(0))]);
    run.goodbye();
    let waited = |c: &Option<Conversation>| {
        c.as_ref()
            .is_some_and(|c| c.waits_for == id(buddy) && c.joined.is_empty())
    };
    let both = [AGAIN, GOODBYE].map(u32::from).to_vec();
    Ok(vec![
        (
            "interjection_joined_steps_out",
            check(
                stepped.0.as_ref().is_some_and(|c| c.joined.is_empty())
                    && joined(&stepped.1, &[id(buddy2)])
                    && on_hold.0.as_ref().is_some_and(|c| c.joined == [id(buddy2)])
                    && on_hold
                        .1
                        .as_ref()
                        .is_some_and(|c| c.waits_for == id(buddy2)),
                format!("unlocked: {stepped:?}; locked: {on_hold:?}"),
            ),
        ),
        (
            "waited_line_offers_anew",
            check(
                offered_first.as_ref() == Some(&both)
                    && offered_then == Some(vec![u32::from(GOODBYE)]),
                format!("as it began to wait {offered_first:?}, as it played {offered_then:?}"),
            ),
        ),
        (
            "interjection_pulls_in",
            check(
                cut && joined(&pulled_in, &[id(buddy)]) && pulled_chimed && moved < 0.05,
                format!(
                    "the guest's cut short {cut}, then {pulled_in:?}, said {pulled_chimed}, the companion moved {moved:.2} m"
                ),
            ),
        ),
        (
            "interjection_waits",
            check(
                waited(&waiting.0) && !waiting.1 && joined(&played.0, &[id(buddy)]) && played.1,
                format!("waiting {waiting:?}, then {played:?}"),
            ),
        ),
        (
            "interjection_wait_capped",
            check(
                !before_cap && joined(&after_cap.0, &[]) && after_cap.1 && guest_on,
                format!(
                    "said before the cap {before_cap}, after it {after_cap:?}, the guest's still up {guest_on}"
                ),
            ),
        ),
    ])
}

/// R7, R11, R12, both guests driving companions.
/// - Damage to the guest's companion cuts its own conversation short,
///   locked or not; joined to the host's, it steps out, the host's going
///   on; a party conversation goes on.
/// - One whose owner is not interruptable goes on; so does one between two
///   others, its listener hurt.
/// - The host down while the guest talks: the guest's goes on, though the
///   host's character is its PC (R12: its owner or listener).
/// - The guest dropping: its own unlocked conversation is cut short; a
///   locked one becomes the host's (the party's, its character
///   listening); joined to another's, it steps out, its request going.
/// - The guest dropping from a locked one while the host stands in its
///   own locked one: the guest's waits, on hold, and becomes the host's
///   once the host's ends; the host's is never cut short. The guest back
///   first has it back, its line not shown again.
/// - The host's choice of party benching that companion while the guest
///   talks is refused, the guest named, the screen kept open; once the
///   guest is free, a choice keeping it closes the screen.
fn interruptions(run: &mut Run) -> Result<Vec<Case>, String> {
    use crate::script::party::{Npc, Selection};
    let (host, buddy, buddy2) = (run.body(0)?, run.body(1)?, run.body(2)?);
    let id = |who: SimId| ObjectId(who.0);
    let aborted =
        |run: &Run| run.last.talk.tokens.get(&ABORT_TOKEN).map(String::as_str) == Some("1");
    run.put(host, HOST_ASIDE);
    // Damage.
    run.forget_tokens();
    let own = run.start(1, TALKER, "probe_talk")?;
    run.edit(own, |c| c.locked = true);
    run.hurt(buddy);
    let hurt_cut = cut_for(run, Some(own), "its listener was hurt") && aborted(run);
    run.click(0, TALK_FROM, 0.0)?;
    run.click(1, TALK_BESIDE, 90.0)?;
    let hosts = run.talk().map(|c| c.id);
    let came = run.talk().is_some_and(|c| c.joined == [id(buddy)]);
    run.hurt(buddy);
    let stepped_out = hosts
        .and_then(|talk| run.conversation(talk))
        .is_some_and(|c| c.joined.is_empty());
    run.goodbye();
    run.cutscene()?;
    let scene = run.talk().map(|c| c.id);
    run.hurt(buddy);
    let party_on = scene.is_some_and(|talk| run.conversation(talk).is_some());
    run.goodbye();
    let captive = run.start(1, CAPTIVE, "probe_talk")?;
    run.hurt(buddy);
    let spared = run.conversation(captive).is_some();
    run.goodbye();
    // Two others talking: the one listening hurt, theirs goes on.
    let barker = object(&run.last, BARKER)?;
    run.begin(TALKER, (barker, SimId(ObjectId::INVALID.0)), "probe_talk")?;
    let npcs = run.last.talk.holding(id(barker)).map(|c| c.id);
    run.hurt(barker);
    let npcs_on = npcs.and_then(|talk| run.conversation(talk)).is_some();
    if let Some(talk) = npcs {
        run.forget(talk);
    }
    // The host down while the guest talks (its PC): the guest's goes on.
    let guests = run.start(1, TALKER, "probe_talk")?;
    run.down_a_step(host);
    let host_down = run.conversation(guests).is_some();
    run.goodbye();
    // Drops.
    run.forget_tokens();
    run.click(1, TALK_FROM, 0.0)?;
    let unlocked = run.talk().map(|c| c.id);
    run.drop_seat(1);
    let drop_cut = cut_for(run, unlocked, "its player dropped") && aborted(run);
    run.rejoin(1, BUDDY)?;
    let back = run.body(1)? == buddy;
    let locked = run.start(1, TALKER, "probe_talk")?;
    run.edit(locked, |c| c.locked = true);
    run.drop_seat(1);
    let handed = run.conversation(locked);
    run.rejoin(1, BUDDY)?;
    run.goodbye();
    run.click(1, TALK_FROM, 0.0)?;
    let asked = run.talk().map_or(u32::MAX, |c| c.id);
    run.edit(asked, |c| {
        c.joined.push(id(buddy2));
        c.asking = id(buddy2);
    });
    run.drop_seat(2);
    let left = run.conversation(asked);
    run.rejoin(2, BUDDY2)?;
    run.goodbye();
    let hosts = run.start(0, HOOKED, "probe_talk")?;
    run.edit(hosts, |c| c.locked = true);
    let guests = run.start(1, TALKER, "probe_talk")?;
    run.edit(guests, |c| c.locked = true);
    run.drop_seat(1);
    let held = (run.conversation(hosts).is_some(), run.conversation(guests));
    run.answers(&[(0, Pick::Reply(GOODBYE))]);
    let taken = run.conversation(guests);
    run.rejoin(1, BUDDY)?;
    run.goodbye();
    // The guest back before the host is free: its own again, as it was.
    let hosts = run.start(0, HOOKED, "probe_talk")?;
    run.edit(hosts, |c| c.locked = true);
    let guests = run.start(1, TALKER, "probe_talk")?;
    run.edit(guests, |c| c.locked = true);
    let shown = run.conversation(guests).map(|c| c.shown_ms);
    run.drop_seat(1);
    run.rejoin(1, BUDDY)?;
    let regained = run.conversation(guests);
    run.goodbye();
    // Benching the companion the guest drives while it talks.
    let mut state = run.world.script_state().clone();
    let before = state.party.clone();
    let table = &mut state.party;
    let npc = Npc {
        template: String::new(),
        object: Some(buddy.0),
        kept: None,
        template_data: None,
    };
    table.available.insert(0, npc);
    table.members = vec![0];
    table.selection = Some(Selection {
        exit: String::new(),
        forced: Vec::new(),
    });
    run.world.set_script_state(state);
    run.click(1, TALK_FROM, 0.0)?;
    let choose = |picks: [u8; 2]| {
        let mut input = TickInput::default();
        input.act(Seat(0), Action::ChooseParty(Some(picks)));
        input
    };
    run.step_with(choose([u8::MAX; 2]));
    let told = run.notices.iter().any(|(at, seat, notice)| {
        *at == run.tick && *seat == Seat(0) && *notice == Notice::Said("buddy is busy.".into())
    });
    let table = &run.world.script_state().party;
    let refused = (told, table.has(0), table.selection.is_some());
    run.goodbye();
    run.step_with(choose([0, u8::MAX]));
    let table = &run.world.script_state().party;
    let free = (table.has(0), table.selection.is_some());
    let mut state = run.world.script_state().clone();
    state.party = before;
    run.world.set_script_state(state);
    run.step(&[]);
    Ok(vec![
        (
            "damage_cuts_private",
            check(
                hurt_cut && came && stepped_out && party_on,
                format!(
                    "locked own cut short {hurt_cut}; joined {came}, stepped out {stepped_out}; party one on {party_on}"
                ),
            ),
        ),
        (
            "damage_spares_uninterruptable",
            check(spared, format!("still up {spared}")),
        ),
        (
            "damage_spares_npcs_talk",
            check(
                npcs.is_some() && npcs_on,
                format!("{npcs:?} still up {npcs_on}"),
            ),
        ),
        (
            "host_down_spares_guests",
            check(host_down, format!("the guest's still up {host_down}")),
        ),
        (
            "drop_unlocked",
            check(
                drop_cut && back,
                format!("cut short {drop_cut}, the companion the guest's again {back}"),
            ),
        ),
        (
            "drop_locked_to_host",
            check(
                handed.as_ref().is_some_and(|c| {
                    c.party && c.locked && c.listener == id(host) && !c.holds(id(buddy))
                }),
                format!("{handed:?}"),
            ),
        ),
        (
            "drop_waits_on_host_locked",
            check(
                held.0
                    && held.1.as_ref().is_some_and(|c| {
                        c.waits_for == id(host)
                            && c.on_hold()
                            && !c.party
                            && c.listener == id(buddy)
                    })
                    && taken.as_ref().is_some_and(|c| {
                        c.party && c.listener == id(host) && c.waits_for == ObjectId::INVALID
                    }),
                format!("the host's up, the guest's: {held:?}; after the host's ends: {taken:?}"),
            ),
        ),
        (
            "drop_back_before_host",
            check(
                regained.as_ref().is_some_and(|c| {
                    c.listener == id(buddy) && !c.party && !c.on_hold() && Some(c.shown_ms) == shown
                }),
                format!("{regained:?}, shown at {shown:?}"),
            ),
        ),
        (
            "drop_clears_request",
            check(
                left.as_ref().is_some_and(|c| {
                    c.listener == id(buddy) && c.joined.is_empty() && c.asking == ObjectId::INVALID
                }),
                format!("{left:?}"),
            ),
        ),
        (
            "bench_refused_while_busy",
            check(
                refused == (true, true, true) && free == (true, false),
                format!("told, kept, screen open: {refused:?}; free, kept and closed: {free:?}"),
            ),
        ),
    ])
}

/// R9, R12: the guest's companion deals itself a match at the dealer
/// (`probe_deal`, no wager), every seat playing a body.
/// - The host talks on its own meanwhile: the match plays on, the guest's
///   moves taking it on, the host's screen showing none.
/// - A party conversation pauses it: the guest's move takes nothing; once
///   it ends, the match plays on.
/// - Hurt in a fight, it is paused until the companion is out of combat.
/// - The guest's seat dropped: paused while the seat is held for it, played
///   on once the guest is back; away for good (`KEEP_MS`), it ends as a
///   quit, the wager held back.
fn matches(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    let stage = |run: &Run| {
        run.table(buddy)
            .map(|table| (table.phase, table.paused.is_some()))
    };
    run.put(host, HOST_ASIDE);
    // Another player talks: the host, near the table, watches it at once
    // (R9a), and leaves to talk.
    run.click(1, DEAL_FROM, 180.0)?;
    run.watch(&[0], None);
    run.click(0, TALK_FROM, 0.0)?;
    let own = run
        .last
        .talk
        .holding(ObjectId(host.0))
        .is_some_and(|c| !c.party);
    run.advise(1);
    let talking = (stage(run), run.last.pazaak_of(Seat(0)).is_some());
    run.goodbye();
    run.leave_table(1);
    // A cutscene.
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.cutscene()?;
    run.advise(1);
    let gathered = stage(run);
    // Ended in the step's dialogue pass, after its pazaak pass: on the next.
    run.goodbye();
    run.step(&[]);
    let after = stage(run);
    run.advise(1);
    let resumed = stage(run);
    run.leave_table(1);
    // A fight.
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.in_combat(buddy, true);
    run.hurt(buddy);
    let hurt = stage(run);
    run.idle(steps(1000));
    let fighting = stage(run);
    run.in_combat(buddy, false);
    run.step(&[]);
    let calm = stage(run);
    run.leave_table(1);
    // Drops.
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    let gold = run.world.script_state().gold;
    run.stake(buddy, 20);
    run.drop_seat(1);
    let away = run
        .table(buddy)
        .map(|table| (table.paused.is_some(), table.away.is_some()));
    run.rejoin(1, BUDDY)?;
    let back = run
        .table(buddy)
        .map(|table| (table.paused.is_some(), table.away.is_some()));
    run.silent = vec![1];
    run.idle(steps(crate::world::drivers::KEEP_MS) - 2);
    let held = stage(run);
    run.idle(4);
    let quit = (run.table(buddy).is_some(), run.world.script_state().gold);
    run.silent.clear();
    run.rejoin(1, BUDDY)?;
    let setup = Some((Stage::Setup, false));
    let play = Some((Stage::Play, false));
    Ok(vec![
        (
            "pazaak_while_other_talks",
            check(
                own && talking == (play, false),
                format!("the host's own up {own}; the guest's match {talking:?}"),
            ),
        ),
        (
            "cutscene_pauses_match",
            check(
                gathered == Some((Stage::Setup, true)) && after == setup && resumed == play,
                format!("in the cutscene {gathered:?}, after {after:?}, a move {resumed:?}"),
            ),
        ),
        (
            "hurt_pauses_match",
            check(
                hurt == Some((Stage::Setup, true)) && fighting == hurt && calm == setup,
                format!("hurt {hurt:?}, fighting on {fighting:?}, out of combat {calm:?}"),
            ),
        ),
        (
            "drop_pauses_then_quits",
            check(
                away == Some((true, true))
                    && back == Some((false, false))
                    && held == Some((Stage::Setup, true))
                    && quit == (false, gold),
                format!(
                    "dropped {away:?}, back {back:?}, away nearly a minute {held:?}; then under way, credits: {quit:?} of {gold}"
                ),
            ),
        ),
    ])
}

/// R9, R12, the guest's companion at the table again.
/// - A cutscene pauses the match while the companion is in a fight it was
///   never hurt in: the cutscene over, it plays on, fight or not.
/// - The guest's match began first, then the host's own; the guest drops:
///   the host's screen still shows its own table, and its moves take.
/// - The guest's match's end script begins a conversation: the guest's
///   own (`Event::asker`), not the party's.
/// - The companion leaves the party mid-match: it ends as a quit at once,
///   the wager held back.
/// - Saved mid-pause and loaded, through the save's bytes: the match
///   resumes as the one not saved does, the same timers to the same end.
fn match_rules(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    let dealer = object(&run.last, DEALER)?;
    let paused = |run: &Run, who: SimId| run.table(who).map(|table| table.paused.is_some());
    run.put(host, HOST_ASIDE);
    // A pause's own cause.
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.in_combat(buddy, true);
    // Begun in the step's dialogue pass, after its pazaak pass: on the next.
    run.cutscene()?;
    run.step(&[]);
    let gathered = paused(run, buddy);
    run.goodbye();
    run.step(&[]);
    let unhurt = paused(run, buddy);
    run.in_combat(buddy, false);
    run.leave_table(1);
    // A dropped guest's table and the host's.
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.put(buddy, BUDDY_AT);
    run.watch(&[0], None);
    run.click(0, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.silent = vec![1];
    run.step(&[]);
    let shown = run.last.pazaak_of(Seat(0)).map(|table| table.player);
    run.advise(0);
    let played = run.table(host).map(|table| table.phase);
    run.silent.clear();
    run.rejoin(1, BUDDY)?;
    run.leave_table(0);
    run.leave_table(1);
    // The end script's conversation.
    crate::script::deal_pazaak(&mut run.world, (buddy.0, dealer.0), 0, "probe_begin", 0);
    run.play(1, Move::Quit);
    let begun = run
        .last
        .talk
        .holding(ObjectId(buddy.0))
        .map(|c| (c.party, c.listener == ObjectId(buddy.0)));
    run.goodbye();
    // Left the party.
    let gold = run.world.script_state().gold;
    crate::script::deal_pazaak(&mut run.world, (buddy.0, buddy.0), 0, "", 0);
    run.stake(buddy, 20);
    run.follow(&[(BUDDY, Some(Seat(1)))], None)?;
    run.step(&[]);
    let left = (run.table(buddy).is_some(), run.world.script_state().gold);
    run.rejoin(1, BUDDY)?;
    // Saved mid-pause.
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.advise(1);
    run.cutscene()?;
    run.step(&[]);
    let saved_paused = paused(run, buddy);
    let mut loaded = run.reloaded()?;
    let mut ends = Vec::new();
    for run in [&mut *run, &mut loaded] {
        run.idle(steps(2000));
        run.goodbye();
        run.step(&[]);
        let due = run.table(buddy).map(|table| table.due_ms);
        run.play_out(1, buddy);
        let state = run.world.script_state();
        ends.push((due, state.pazaak_won, state.gold, run.last.hash()));
    }
    let alike = ends[0] == ends[1] && ends[0].0.is_some() && saved_paused == Some(true);
    Ok(vec![
        (
            "pause_ends_with_its_cause",
            check(
                gathered == Some(true) && unhurt == Some(false),
                format!("in the cutscene {gathered:?}, after it in a fight unhurt {unhurt:?}"),
            ),
        ),
        (
            "dropped_table_spares_hosts",
            check(
                shown == Some(host.0) && played == Some(Stage::Play),
                format!("the host's screen shows {shown:?}'s, its move: {played:?}"),
            ),
        ),
        (
            "end_script_talk_is_players",
            check(
                begun == Some((false, true)),
                format!("party, listened by the companion: {begun:?}"),
            ),
        ),
        (
            "left_quits_match",
            check(
                left == (false, gold),
                format!("under way, credits: {left:?} of {gold}"),
            ),
        ),
        (
            "paused_save_resumes_alike",
            check(
                alike,
                format!(
                    "paused when saved {saved_paused:?}; resumed due, won, gold, hash: {ends:?}"
                ),
            ),
        ),
    ])
}

/// R9a, the guest's companion dealt a match at the dealer.
/// - Scenario 37: the host stands free 4 m off, the second guest's
///   companion fights beyond 10 m: the host watches at once, not the other.
/// - Scenario 38: the second guest's companion uses the one at the table:
///   in a fight, not free to watch, its use passes to the dealer behind;
///   out of it, it watches too, after the host.
/// - A watcher is held where it stands, facing the table's player, and is
///   not busy: no icon, no match of its own, nothing keeping it from an
///   exit, and its ready for one taken, which stops its watching.
/// - Saved mid-watch and loaded, through the save's bytes: the same
///   watchers, and it goes on alike.
/// - Scenario 40: a hurt stops a watcher's watching, and a cutscene every
///   watcher's.
/// - Scenario 39: the host's Leave stops its watching, as `Action::Watch`
///   with no player does the second guest's; the host deals itself a match,
///   which the second guest watches, the guest's going on.
/// - In one step the host uses the guest's companion at the table and the
///   second guest sends `Action::Watch`: both watch, in seat order. In
///   another the host and the second guest ask to watch, and the second
///   guest's companion uses the dealer: both asks are taken (the pazaak pass
///   goes first), then its own match, dealt in the dialogue pass, stops its
///   watching.
fn watch(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy, buddy2) = (run.body(0)?, run.body(1)?, run.body(2)?);
    let dealer = object(&run.last, DEALER)?;
    let watchers = |run: &Run, who: SimId| run.table(who).map(|table| table.watchers.clone());
    let watching = |run: &Run, seat: u8| run.last.watching(Seat(seat)).map(|table| table.player);
    // Scenario 37.
    run.put(host, HOST_ASIDE);
    run.put(buddy2, FAR);
    run.in_combat(buddy2, true);
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    let near = (watchers(run, buddy), watching(run, 0), watching(run, 2));
    // Scenario 38. In a fight, its use passes the table to the dealer
    // behind it, whose line deals it a match of its own: it leaves that.
    run.click(2, WATCH_FROM, 180.0)?;
    let reached: Vec<SimId> = (run.world.uses_heard().iter())
        .filter(|heard| heard.by == buddy2)
        .map(|heard| heard.object)
        .collect();
    let fighting = (watchers(run, buddy), reached);
    run.goodbye();
    run.leave_table(2);
    run.in_combat(buddy2, false);
    run.click(2, WATCH_FROM, 180.0)?;
    let used = (watchers(run, buddy), watching(run, 2));
    // Held and not busy.
    let held = run.held();
    run.idle(4);
    let faced = [host, buddy2].map(|watcher| faces_off(run, watcher, buddy));
    let busy = [0, 2].map(|seat| {
        let last = &run.last;
        let seat = Seat(seat);
        (
            last.busy(seat),
            last.pazaak_of(seat).is_some(),
            last.kept_from_exit(seat),
        )
    });
    // Saved mid-watch.
    let mut loaded = run.reloaded()?;
    let saved = (
        loaded.world.script_state().pazaak == run.world.script_state().pazaak,
        loaded.last.pazaak == run.last.pazaak,
    );
    let mut hashes = Vec::new();
    for run in [&mut *run, &mut loaded] {
        run.idle(8);
        hashes.push(run.last.hash());
    }
    // An exit's ready check.
    run.propose()?;
    let readied = (run.ready(&[2]), watching(run, 2), watching(run, 0));
    run.put(host, HOST_ASIDE);
    run.step(&[]);
    let lapsed = run.last.ready.is_none();
    // Scenario 40.
    run.watch(&[2], Some(buddy));
    run.hurt(host);
    let hurt = (watching(run, 0), watching(run, 2));
    run.watch(&[0], Some(buddy));
    let both = watchers(run, buddy);
    run.cutscene()?;
    let stopped_at_once = watchers(run, buddy);
    // Paused in the step's pazaak pass, before its dialogue pass: on the next.
    run.step(&[]);
    let gathered = (
        stopped_at_once,
        run.table(buddy).map(|t| t.paused.is_some()),
    );
    run.goodbye();
    // Scenario 39.
    run.watch(&[0, 2], Some(buddy));
    run.answers(&[(0, Pick::Leave)]);
    let left = (watching(run, 0), watching(run, 2));
    run.watch(&[2], None);
    let stopped = watching(run, 2);
    run.put(buddy, BUDDY_AT);
    run.click(0, DEAL_FROM, 180.0)?;
    run.goodbye();
    let dealt = (
        watchers(run, host),
        watchers(run, buddy),
        run.table(buddy).is_some(),
    );
    run.leave_table(0);
    run.leave_table(1);
    // One step: two asks and a deal.
    run.put(host, FAR);
    run.put(buddy2, FAR_TOO);
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    let unseen = watchers(run, buddy);
    // The host's use is heard after the second guest's action, yet comes
    // first: seat order.
    run.idle(USE_GAP);
    run.uses_with(&[(0, WATCH_FROM, 180.0)], asked_to_watch(&[2], Some(buddy)))?;
    let in_order = watchers(run, buddy);
    run.watch(&[0, 2], None);
    run.put(buddy, BUDDY_AT);
    run.idle(USE_GAP);
    run.uses_with(
        &[(2, DEAL_FROM, 180.0)],
        asked_to_watch(&[2, 0], Some(buddy)),
    )?;
    let same_step = (
        watchers(run, buddy),
        watching(run, 2),
        watchers(run, buddy2),
    );
    run.goodbye();
    run.leave_table(2);
    run.leave_table(1);
    let gone = (0..SEATS).all(|seat| run.last.watching(Seat(seat)).is_none());
    let none = Some(Vec::new());
    Ok(vec![
        (
            "free_near_watch",
            check(
                near == (Some(vec![host.0]), Some(buddy.0), None),
                format!("watchers, the host's and the second guest's: {near:?}"),
            ),
        ),
        (
            "use_to_watch",
            check(
                fighting == (Some(vec![host.0]), vec![dealer])
                    && used == (Some(vec![host.0, buddy2.0]), Some(buddy.0)),
                format!(
                    "used in a fight (watchers, what it reached) {fighting:?}; out of it {used:?}"
                ),
            ),
        ),
        (
            "watcher_held_and_not_busy",
            check(
                [host, buddy2]
                    .iter()
                    .all(|who| held.get(who) == Some(&true))
                    && faced
                        .iter()
                        .all(|off| off.as_ref().is_ok_and(|off| *off < 2.0))
                    && busy == [(None, false, None); 2]
                    && readied == (Some(0b101), None, Some(buddy.0))
                    && lapsed,
                format!(
                    "held {held:?}, degrees off {faced:?}, busy {busy:?}; the second guest readied (ready, its watching, the host's) {readied:?}, lapsed {lapsed}"
                ),
            ),
        ),
        (
            "watch_round_trip",
            check(
                saved == (true, true) && hashes[0] == hashes[1],
                format!(
                    "the scripts' state and the snapshot alike {saved:?}; on alike {:?}",
                    hashes.iter().map(SnapshotHash::short).collect::<Vec<_>>()
                ),
            ),
        ),
        (
            "hurt_stops_watching",
            check(
                hurt == (None, Some(buddy.0)),
                format!("after the host's hurt, the host's and the second guest's: {hurt:?}"),
            ),
        ),
        (
            "cutscene_stops_watching",
            check(
                both == Some(vec![buddy2.0, host.0]) && gathered == (none.clone(), Some(true)),
                format!("watchers {both:?}, in a cutscene (and paused) {gathered:?}"),
            ),
        ),
        (
            "leave_to_deal",
            check(
                left == (None, Some(buddy.0))
                    && stopped.is_none()
                    && dealt == (Some(vec![buddy2.0]), none.clone(), true),
                format!(
                    "the host left {left:?}, the second guest stopped {stopped:?}; the host's watchers, the guest's, the guest's under way: {dealt:?}"
                ),
            ),
        ),
        (
            "same_step_watch_and_deal",
            check(
                unseen == none
                    && in_order == Some(vec![host.0, buddy2.0])
                    && same_step == (Some(vec![host.0]), None, none.clone())
                    && gone,
                format!(
                    "none near {unseen:?}, a use and an action in one step {in_order:?}; the guest's watchers, the second guest's watching, its own's watchers: {same_step:?}; none after {gone}"
                ),
            ),
        ),
    ])
}

/// R9a's rulings R35 and R36, and a Leave by seat.
/// - The host leading through the third, the third is pulled in to watch,
///   and the host's Leave (its seat's) stops it.
/// - R36: the third, leading, deals itself a match; the second guest uses
///   it at the table and watches.
/// - R35: the guest's companion joins the host's dealer conversation; the
///   host takes "Again." and its line deals the host a match: the guest's
///   companion steps out of the conversation and watches it.
/// - R37: the guest's companion joins the host's dealer conversation and
///   takes "Again." itself: its match is dealt, the host still talking;
///   once that conversation ends, the host watches it; far off as it
///   ends, it does not.
fn watch_more(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy, buddy2) = (run.body(0)?, run.body(1)?, run.body(2)?);
    let aide = object(&run.last, AIDE)?;
    let watchers = |run: &Run, who: SimId| run.table(who).map(|table| table.watchers.clone());
    // Leading through the third.
    run.lead(Some(AIDE))?;
    run.put(buddy2, FAR);
    run.put(aide, HOST_ASIDE);
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    let lent_pulled = watchers(run, buddy);
    run.answers(&[(0, Pick::Leave)]);
    let lent_left = watchers(run, buddy);
    run.leave_table(1);
    run.put(buddy, FAR_TOO);
    run.click(0, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.click(2, WATCH_FROM, 180.0)?;
    let lent_used = watchers(run, aide);
    run.leave_table(0);
    run.lead(None)?;
    // R35.
    run.put(aide, AIDE_AT);
    run.put(buddy2, FAR);
    run.click(0, DEAL_FROM, 180.0)?;
    run.play(0, Move::Quit);
    run.idle(steps(crate::pazaak::END_MS) + 1);
    let quit = run.table(host).is_none();
    run.click(1, DEAL_BESIDE, 90.0)?;
    let joined = (run.last.talk.holding(ObjectId(host.0))).map(|c| c.joined.clone());
    run.answers(&[(0, Pick::Reply(AGAIN))]);
    let dealt = (
        watchers(run, host),
        run.last.talk.holding(ObjectId(buddy.0)).is_some(),
    );
    run.goodbye();
    run.leave_table(0);
    // R37 (`Run::guest_deals`), then again, the host far off as it ends.
    let guest_deals = |run: &mut Run| -> Result<_, String> {
        run.guest_deals()?;
        Ok((
            watchers(run, buddy),
            run.last.talk.holding(ObjectId(host.0)).is_some(),
        ))
    };
    let guest_dealt = guest_deals(run)?;
    run.goodbye();
    let talk_ended = watchers(run, buddy);
    run.leave_table(1);
    let far_dealt = guest_deals(run)?;
    run.put(host, FAR);
    let far_talking = run.last.talk.holding(ObjectId(host.0)).is_some();
    run.goodbye();
    let far_ended = watchers(run, buddy);
    run.leave_table(1);
    Ok(vec![
        (
            "leave_while_leading",
            check(
                lent_pulled == Some(vec![aide.0]) && lent_left == Some(Vec::new()),
                format!(
                    "the third pulled in {lent_pulled:?}, after the host's Leave {lent_left:?}"
                ),
            ),
        ),
        (
            "lent_player_used_to_watch",
            check(
                lent_used == Some(vec![buddy2.0]),
                format!("the third's watchers after the second guest's use {lent_used:?}"),
            ),
        ),
        (
            "joined_dealer_talk_watches",
            check(
                quit && joined == Some(vec![ObjectId(buddy.0)])
                    && dealt == (Some(vec![buddy.0]), false),
                format!(
                    "quit {quit}, joined {joined:?}; the host's watchers, the guest's companion still in it: {dealt:?}"
                ),
            ),
        ),
        (
            "dealer_talk_ended_far_not",
            check(
                far_dealt == (Some(Vec::new()), true)
                    && far_talking
                    && far_ended == Some(Vec::new()),
                format!(
                    "dealt {far_dealt:?}; the host far off, still talking {far_talking}; once it ends {far_ended:?}"
                ),
            ),
        ),
        (
            "dealer_talk_ended_watches",
            check(
                guest_dealt == (Some(Vec::new()), true) && talk_ended == Some(vec![host.0]),
                format!(
                    "the guest's watchers, the host still talking: {guest_dealt:?}; once it ends {talk_ended:?}"
                ),
            ),
        ),
    ])
}

/// R9a's rulings R39 and R40, one Leave, and a mark stepping out.
/// - R39: the guest's companion deals itself a match, the host watching;
///   the guest's seat drops: the match paused away, the host still watches
///   it (its screen keeps the table, `pazaak_view`), and its Leave stops it.
/// - R39: a fight that reaches a watcher (its own) stops its watching.
/// - R40: the guest's companion takes the dealing reply in the host's
///   conversation (`Run::guest_deals`), and the host leaves that
///   conversation, cut short: it does not watch.
/// - Leading through the third, the host's character talking, the third
///   is pulled in to watch: the host's Leave stops the watching alone, its
///   character's conversation going on.
/// - The guest's companion, joined to the host's conversation, marks a
///   reply; a while after, its line deals the host a match (`probe_later`):
///   the companion steps out to watch, its mark going with it.
/// - R41: the host, free, uses the guest's companion at its table: it
///   watches, the same after a save loaded the step before (the same
///   hash), and a client predicting the use meets the same body.
fn watch_held(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy, buddy2) = (run.body(0)?, run.body(1)?, run.body(2)?);
    let aide = object(&run.last, AIDE)?;
    let watchers = |run: &Run, who: SimId| run.table(who).map(|table| table.watchers.clone());
    let watching = |run: &Run, seat: u8| run.last.watching(Seat(seat)).map(|table| table.player);
    let talk_of = |run: &Run, who: SimId| run.last.talk.holding(ObjectId(who.0)).map(|c| c.id);
    // R39: the player's seat drops.
    run.put(host, HOST_ASIDE);
    run.put(buddy2, FAR);
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.silent = vec![1];
    run.step(&[]);
    let away = (
        run.table(buddy)
            .map(|table| (table.paused.is_some(), table.away.is_some())),
        watching(run, 0),
    );
    run.answers(&[(0, Pick::Leave)]);
    let left = watching(run, 0);
    run.silent.clear();
    run.rejoin(1, BUDDY)?;
    // R39: a fight reaches the watcher.
    run.watch(&[0], Some(buddy));
    let again = watching(run, 0);
    run.in_combat(host, true);
    run.step(&[]);
    let fought = watching(run, 0);
    run.in_combat(host, false);
    run.leave_table(1);
    // R40.
    run.guest_deals()?;
    let dealt = watchers(run, buddy);
    run.answers(&[(0, Pick::Leave)]);
    let cut = (talk_of(run, host), watchers(run, buddy));
    run.goodbye();
    run.leave_table(1);
    // One Leave: the host's character talking, then leading through the
    // third.
    run.put(buddy2, FAR);
    run.put(host, TALK_FROM);
    let own = run.start(0, TALKER, "probe_talk")?;
    run.lead(Some(AIDE))?;
    run.put(aide, HOST_ASIDE);
    run.click(1, DEAL_FROM, 180.0)?;
    for _ in 0..4 {
        if talk_of(run, buddy).is_some() {
            run.answers(&[(1, Pick::Reply(GOODBYE))]);
        }
    }
    let pulled = watchers(run, buddy);
    run.answers(&[(0, Pick::Leave)]);
    let one_leave = (watchers(run, buddy), run.conversation(own).is_some());
    run.goodbye();
    run.leave_table(1);
    run.lead(None)?;
    // A mark stepping out.
    run.begin(DEALER, (host, host), "probe_deal_later")?;
    let later = talk_of(run, host);
    run.click(1, DEAL_BESIDE, 90.0)?;
    run.answers(&[(1, Pick::Point(AGAIN))]);
    let marks = |run: &Run| later.and_then(|id| run.conversation(id)).map(|c| c.marks);
    let marked = marks(run);
    run.idle(steps((LATER_S * 1000.0) as u64));
    let stepped_out = (watchers(run, host), marks(run));
    run.goodbye();
    run.leave_table(0);
    // R41: the host free and far off as the guest's companion deals itself
    // a match; then its use at the table, on, after a save loaded the step
    // before, and as a client predicts it.
    run.put(host, FAR);
    run.put(buddy2, FAR_TOO);
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.idle(USE_GAP);
    let mut loaded = run.reloaded()?;
    let mut predicted = run.predicted()?;
    let mut reached = Vec::new();
    for each in [&mut *run, &mut loaded, &mut predicted] {
        each.uses(&[(0, WATCH_FROM, 180.0)])?;
        reached.push(
            (each.world.uses_heard().iter())
                .filter(|heard| heard.by == host)
                .map(|heard| heard.object)
                .collect::<Vec<SimId>>(),
        );
    }
    let alike = (
        [&*run, &loaded].map(|run| watching(run, 0)),
        run.last.hash() == loaded.last.hash(),
    );
    run.watch(&[0], None);
    run.leave_table(1);
    Ok(vec![
        (
            "use_at_table_loaded_alike",
            check(
                reached.iter().all(|met| *met == [buddy])
                    && reached.len() == 3
                    && alike == ([Some(buddy.0); 2], true),
                format!(
                    "what the host's use met, on, loaded, predicted: {reached:?}; watching (on, loaded), hashes alike: {alike:?}"
                ),
            ),
        ),
        (
            "paused_watcher_watches_and_leaves",
            check(
                away == (Some((true, true)), Some(buddy.0)) && left.is_none(),
                format!(
                    "the guest dropped (paused, away), the host watching: {away:?}; after its Leave {left:?}"
                ),
            ),
        ),
        (
            "fight_stops_watching",
            check(
                again == Some(buddy.0) && fought.is_none(),
                format!("the host watching {again:?}; in a fight {fought:?}"),
            ),
        ),
        (
            "listener_leave_not_watching",
            check(
                dealt == Some(Vec::new()) && cut == (None, Some(Vec::new())),
                format!(
                    "the guest's watchers as dealt {dealt:?}; the host left (talking, watchers) {cut:?}"
                ),
            ),
        ),
        (
            "one_leave_stops_watching_alone",
            check(
                pulled == Some(vec![aide.0]) && one_leave == (Some(Vec::new()), true),
                format!(
                    "the character talking in {own}, the third pulled in {pulled:?}; after the Leave (watchers, still talking) {one_leave:?}"
                ),
            ),
        ),
        (
            "stepped_out_mark_gone",
            check(
                marked == Some(BTreeMap::from([(1, AGAIN)]))
                    && stepped_out == (Some(vec![buddy.0]), Some(BTreeMap::new())),
                format!("marked {marked:?}; once dealt (watchers, marks) {stepped_out:?}"),
            ),
        ),
    ])
}

/// R11, P16: the host walks into the way out while the guests' companions
/// are busy, every seat playing a body.
/// - The second guest readies, then its companion deals itself a match:
///   it is unready again.
/// - The first guest's companion, at the table or in a locked
///   conversation, readies: it stays unready, the party waits, and the
///   check says why.
/// - In its own unlocked conversation and trading, it readies with the
///   second guest: it leaves both (the conversation cut short once, as its
///   Leave cuts it), and the party goes.
fn exits(run: &mut Run) -> Result<Vec<Case>, String> {
    let (buddy, buddy2) = (run.body(1)?, run.body(2)?);
    let why = |run: &Run, seat: u8| run.last.kept_from_exit(Seat(seat));
    let host_only = Some(0b001);
    // The host's request keys wait while a ready check is up or the host is
    // down (R5; the ready key is the accept key by default).
    let downed = |snapshot: &Snapshot, who: SimId| {
        let mut snapshot = snapshot.clone();
        let fighter = snapshot
            .entities
            .iter_mut()
            .find(|e| e.id == who)
            .and_then(|e| e.fighter.as_mut());
        if let Some(fighter) = fighter {
            fighter.life = crate::combat::fighter::Life::Downed;
        }
        snapshot
    };
    let host = run.body(0)?;
    let mut waits = vec![
        run.last.request_wait(),
        downed(&run.last, host).request_wait(),
    ];
    run.propose()?;
    waits.push(run.last.request_wait());
    let readied = run.ready(&[2]);
    // The check reads the matches and conversations as the last step's
    // scripts left them: a match dealt, or a lock set, between steps counts
    // from the second step after.
    crate::script::deal_pazaak(&mut run.world, (buddy2.0, buddy2.0), 0, "", 0);
    run.idle(2);
    let dealt = (
        run.last.ready.as_ref().map(|check| check.ready),
        why(run, 2),
    );
    let mut icons = vec![run.last.busy(Seat(2))];
    run.leave_table(2);
    run.click(1, DEAL_FROM, 180.0)?;
    run.goodbye();
    let at_table = (run.ready(&[1]), why(run, 1));
    run.leave_table(1);
    let locked = run.start(1, TALKER, "probe_talk")?;
    run.edit(locked, |c| c.locked = true);
    run.step(&[]);
    let in_locked = (run.ready(&[1]), why(run, 1));
    icons.push(run.last.busy(Seat(1)));
    let down_icon = downed(&run.last, buddy).busy(Seat(1));
    run.goodbye();
    run.click(1, SHOP_FROM, 90.0)?;
    icons.push(run.last.busy(Seat(1)));
    let own = run
        .last
        .talk
        .holding(ObjectId(buddy.0))
        .map(|c| (c.id, c.locked, run.trading(buddy)));
    let talk = own.map_or(u32::MAX, |(id, ..)| id);
    let went = (
        run.ready(&[1, 2]),
        run.last.transition.as_ref().map(|t| t.module.clone()),
    );
    let ended = run
        .world
        .script_report()
        .talk
        .iter()
        .filter(|line| matches!(line, Line::Ended { id, aborted: true, .. } if *id == talk))
        .count();
    let left = (run.conversation(talk).is_some(), ended, run.trading(buddy));
    let pazaak = Some(crate::world::party::PLAYING_PAZAAK);
    Ok(vec![
        (
            "ready_waits_on_match",
            check(
                readied == Some(0b101)
                    && dealt == (host_only, pazaak)
                    && at_table == (host_only, pazaak),
                format!(
                    "ready {readied:?}, dealt a match {dealt:?}; at the table, readying {at_table:?}"
                ),
            ),
        ),
        (
            "ready_waits_on_locked",
            check(
                in_locked == (host_only, Some(crate::world::party::IN_CONVERSATION)),
                format!("in a locked conversation, readying {in_locked:?}"),
            ),
        ),
        (
            "ready_leaves_unlocked",
            check(
                own.is_some_and(|(_, locked, trading)| !locked && trading)
                    && went == (None, Some("elsewhere".into()))
                    && left == (false, 1, false),
                format!(
                    "talking (id, locked, trading) {own:?}; readied: {went:?}; up, cut short, trading: {left:?}"
                ),
            ),
        ),
        (
            "busy_pazaak_talking_shopping",
            check(
                icons
                    == [
                        Some(Busy::Pazaak),
                        Some(Busy::Talking),
                        Some(Busy::Shopping),
                    ],
                format!("at the table, in a locked conversation, talking and trading: {icons:?}"),
            ),
        ),
        (
            "busy_none_while_down",
            check(
                down_icon.is_none(),
                format!("down in a locked conversation: {down_icon:?}"),
            ),
        ),
        (
            "request_keys_wait",
            check(
                waits == [None, Some(RequestWait::Down), Some(RequestWait::ReadyCheck)],
                format!("standing, down, a ready check up: {waits:?}"),
            ),
        ),
    ])
}

/// R11, R12, last, as they leave the companion dead and a module change
/// asked for.
/// - The guest asking the host in from a locked conversation drops while
///   the host stands in its own locked one: the guest's waits on the host,
///   its request gone (the host takes it over anyway); a module change
///   then cuts it short, the host's own going on.
/// - The guest's companion killed in its locked conversation: it is cut
///   short (its `EndConverAbort` runs), not handed to the host; its match
///   ends as a quit, the wager held back; its trade ends.
/// - A script asks for another module: the second guest's own
///   conversation is cut short; the host's own goes on, and so does a
///   party one, until the module goes.
fn endings(run: &mut Run) -> Result<Vec<Case>, String> {
    let (host, buddy) = (run.body(0)?, run.body(1)?);
    run.put(host, HOST_ASIDE);
    // The guest asking in its locked one drops while the host stands in its
    // own locked one; then a script asks for another module.
    let hosts = run.start(0, HOOKED, "probe_talk")?;
    run.edit(hosts, |c| c.locked = true);
    let guests = run.start(1, TALKER, "probe_talk")?;
    run.edit(guests, |c| {
        c.locked = true;
        c.asking = ObjectId(buddy.0);
    });
    run.drop_seat(1);
    let waiting = run.conversation(guests).map(|c| (c.waits_for.0, c.asking));
    run.run_script("probe_leave")?;
    let waited_cut = (
        cut_for(run, Some(guests), "the module changed"),
        run.conversation(hosts).is_some(),
    );
    run.rejoin(1, BUDDY)?;
    run.goodbye();
    run.forget_tokens();
    run.shop(&[1])?;
    let shopping = run.trading(buddy);
    let locked = run.start(1, TALKER, "probe_talk")?;
    run.edit(locked, |c| c.locked = true);
    let gold = run.world.script_state().gold;
    crate::script::deal_pazaak(&mut run.world, (buddy.0, buddy.0), 0, "", 0);
    run.stake(buddy, 20);
    run.kill(buddy);
    let shopping = (shopping, run.trading(buddy));
    let died = cut_for(run, Some(locked), "its listener died")
        && run.last.talk.tokens.get(&ABORT_TOKEN).map(String::as_str) == Some("1")
        && !run.last.talk.conversations.iter().any(|c| c.party);
    let quit = (run.table(buddy).is_some(), run.world.script_state().gold);
    run.click(0, TALK_FROM, 0.0)?;
    let hosts = run.talk().map(|c| c.id);
    let guests = run.start(2, HOOKED, "probe_talk")?;
    run.run_script("probe_leave")?;
    let leaving = (
        cut_for(run, Some(guests), "the module changed"),
        hosts.is_some_and(|talk| run.conversation(talk).is_some()),
    );
    run.goodbye();
    run.cutscene()?;
    let scene = run.talk().map(|c| c.id);
    run.run_script("probe_leave")?;
    let party_on = scene.is_some_and(|talk| run.conversation(talk).is_some());
    run.goodbye();
    Ok(vec![
        (
            "death_cuts_locked",
            check(died, format!("cut short, aborted, handed to nobody {died}")),
        ),
        (
            "death_ends_trade",
            check(
                shopping == (true, false),
                format!("the companion trading, then dead: {shopping:?}"),
            ),
        ),
        (
            "death_quits_match",
            check(
                quit == (false, gold),
                format!("under way, credits: {quit:?} of {gold}"),
            ),
        ),
        (
            "waiting_on_host_drops_request",
            check(
                waiting == Some((host.0, ObjectId::INVALID)),
                format!("waits on, asking: {waiting:?}"),
            ),
        ),
        (
            "module_change_cuts_waiting",
            check(
                waited_cut == (true, true),
                format!("the dropped guest's cut short, the host's up: {waited_cut:?}"),
            ),
        ),
        (
            "module_change_cuts_guests",
            check(
                leaving == (true, true) && party_on,
                format!(
                    "the guest's cut short, the host's up: {leaving:?}; a party one up {party_on}"
                ),
            ),
        ),
    ])
}

/// R1: one seat alone plays as before: the host's use of the one who talks
/// leaves the companion (its own mind's) where it stands far off, as in
/// KOTOR (scene mode's S9: only more than one seat gathers); hurt mid-talk, the
/// host talks on (R7 cuts only while another seat plays); its match plays
/// on through a cutscene and a fight (R9); its store stays open hurt and
/// through a cutscene (R8); its ready for an exit leaves neither its
/// conversation nor its store (R11).
fn one_seat_as_before() -> Result<Vec<Case>, String> {
    let mut run = Run::new(1, None)?;
    let (host, buddy) = (run.body(0)?, object(&run.last, BUDDY)?);
    run.idle(USE_GAP);
    run.put(buddy, FAR);
    run.uses(&[(0, TALK_FROM, 0.0)])?;
    let talk = run.talk().cloned();
    let far = between(&run, host, buddy);
    run.hurt(host);
    let kept = talk
        .as_ref()
        .is_some_and(|c| run.conversation(c.id).is_some());
    run.goodbye();
    let ended = run.last.talk.conversations.is_empty();
    // Its match plays on through a cutscene and a fight (R9).
    run.click(0, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.cutscene()?;
    let gathered = run.table(host).map(|table| table.paused.is_some());
    run.goodbye();
    run.in_combat(host, true);
    run.hurt(host);
    let hurt = run.table(host).map(|table| table.paused.is_some());
    run.in_combat(host, false);
    run.leave_table(0);
    run.shop(&[0])?;
    run.hurt(host);
    let shopping = run.trading(host);
    run.cutscene()?;
    let shopping = (shopping, run.trading(host));
    run.goodbye();
    // A proposal standing in a fight (restored so: a lone player's goes at
    // once otherwise), the host at the way out in its own conversation and
    // trading: its ready leaves neither (R11 is for players together).
    let own = run.start(0, TALKER, "probe_talk")?;
    run.fight(true)?;
    run.propose()?;
    let (via, since_ms) = (object(&run.last, WAY_OUT)?, run.last.time_ms);
    run.rebuilt(|snapshot| {
        snapshot.ready = Some(crate::snapshot::ReadyCheck {
            module: "elsewhere".into(),
            tag: String::new(),
            via,
            asked_by: Seat(0),
            ready: 0b1,
            since_ms,
        });
        Ok(())
    })?;
    let readied = run.ready(&[0]);
    let readied = (readied, run.conversation(own).is_some(), run.trading(host));
    // Its conversation locked, the check's banner names no reason either.
    run.edit(own, |c| c.locked = true);
    run.step(&[]);
    let banner = run.last.kept_from_exit(Seat(0));
    run.fight(false)?;
    Ok(vec![
        (
            "one_seat_ready_leaves_nothing",
            check(
                readied == (Some(0b1), true, true),
                format!("in a fight, the host readied: ready, talking, trading {readied:?}"),
            ),
        ),
        (
            "one_seat_banner_keeps_no_one",
            check(
                banner.is_none(),
                format!("in a locked conversation, the banner's reason {banner:?}"),
            ),
        ),
        (
            "one_seat_store_stays_open",
            check(
                shopping == (true, true),
                format!("trading hurt, then in a cutscene {shopping:?}"),
            ),
        ),
        (
            "one_seat_match_plays_on",
            check(
                gathered == Some(false) && hurt == Some(false),
                format!("paused in a cutscene {gathered:?}, hurt in a fight {hurt:?}"),
            ),
        ),
        (
            "one_seat_as_before",
            check(
                talk.as_ref().is_some_and(|c| c.listener.0 == host.0) && far > 10.0 && ended,
                format!("{talk:?}, the companion {far:.2} m from the host, ended {ended}"),
            ),
        ),
        (
            "one_seat_hurt_talks_on",
            check(kept, format!("still up after the host was hurt {kept}")),
        ),
    ])
}

/// R34: the party's combat reach shrunk to 3 m (`ranges.2da` PartyCombat)
/// and the barker fighting, the host stands 1.4 m from it, the second
/// guest's companion 3.6 m, both free and near the guest's companion as its
/// match begins: only the second guest watches, the host beside a fight.
/// Then the second guest stands beside the fight too: it stops watching
/// (R39).
fn fight_near_not_pulled_in() -> Result<Vec<Case>, String> {
    let mut run = Run::on_ground(SEATS, Some(Seat(1)), |data| {
        data.ranges.party_combat_m = 3.0;
    })?;
    run.silent.clear();
    run.follow(&[(BUDDY2, Some(Seat(2)))], Some(Seat(0)))?;
    run.step(&[]);
    let (host, buddy, buddy2) = (run.body(0)?, run.body(1)?, run.body(2)?);
    run.fight(true)?;
    run.put(host, HOST_ASIDE);
    run.put(buddy2, WATCH_FROM);
    run.click(1, DEAL_FROM, 180.0)?;
    let watchers = |run: &Run| run.table(buddy).map(|table| table.watchers.clone());
    let (in_combat, pulled) = (run.last.in_combat, watchers(&run));
    run.put(buddy2, BARK_FROM);
    run.step(&[]);
    let reached = watchers(&run);
    Ok(vec![
        (
            "fight_near_not_pulled_in",
            check(
                in_combat && pulled == Some(vec![buddy2.0]),
                format!("the party in combat {in_combat}, the watchers {pulled:?}"),
            ),
        ),
        (
            "fight_reaching_watcher_stops",
            check(
                reached == Some(Vec::new()),
                format!("beside the fight, the watchers {reached:?}"),
            ),
        ),
    ])
}

/// R9a: one seat alone has no one to watch it. Its companion, its own
/// mind's, stands free beside the table and does not watch; asking to watch
/// its own match does nothing.
fn one_seat_no_watchers() -> Result<Vec<Case>, String> {
    let mut run = Run::new(1, None)?;
    let (host, buddy) = (run.body(0)?, object(&run.last, BUDDY)?);
    run.put(buddy, WATCH_FROM);
    run.click(0, DEAL_FROM, 180.0)?;
    run.goodbye();
    run.watch(&[0], Some(host));
    let watched = (
        run.table(host).map(|table| table.watchers.clone()),
        run.last.watching(Seat(0)).is_some(),
    );
    run.leave_table(0);
    Ok(vec![(
        "one_seat_no_watchers",
        check(
            watched == (Some(Vec::new()), false),
            format!("watchers, the host watching: {watched:?}"),
        ),
    )])
}

/// One seat alone leading through its companion (`world::lent`), which
/// talks for it: the player's character is in that conversation, as it was
/// before conversations kept their listener (`GetIsInConversation(
/// GetFirstPC())` reads true, and the minds see it in the scene,
/// `Told::with` with `world::host_bodies`), and a script's conversation
/// with it is refused, the player's not cut short.
fn one_seat_leading() -> Result<Vec<Case>, String> {
    let mut run = Run::new(1, None)?;
    let buddy = object(&run.last, BUDDY)?;
    run.lead(Some(BUDDY))?;
    run.click(0, TALK_FROM, 0.0)?;
    let lent = run.talk().map(|c| (c.id, c.listener.0 == buddy.0));
    let pc = run.last.own(Seat(0)).map(|own| own.id);
    let host = crate::world::host_bodies(pc, Some(buddy));
    let in_scene = run.last.talk.with(host[0], host).map(|c| c.id);
    run.run_script("probe_busy")?;
    let busy = run.last.talk.tokens.get(&BUSY_TOKEN).cloned();
    let before = refusals(&run, "already talking");
    run.cutscene()?;
    let refused = refusals(&run, "already talking") - before;
    let kept = lent.is_some_and(|(talk, _)| run.conversation(talk).is_some());
    Ok(vec![(
        "one_seat_leading_still_talking",
        check(
            lent.is_some_and(|(talk, through)| through && in_scene == Some(talk))
                && busy.as_deref() == Some("1")
                && refused == 1
                && kept,
            format!(
                "{lent:?}; GetIsInConversation(GetFirstPC()) {busy:?}, in the minds' scene {in_scene:?}; a script's refused {refused}, the player's kept {kept}"
            ),
        ),
    )])
}
