//! Shared conversations (issues 636 and 637), on the proving ground with
//! the host, a guest driving the companion and a guest driving the second
//! companion, every player with "Join nearby conversations" on and the host
//! with "Show chosen replies" on (`Run::share`). They cover:
//! - the first seat's own conversation bringing in a free player near it,
//!   not one out of range, one in a fight, one with a container or a screen
//!   open, or one who turned the option off;
//! - the option kept through a save and a seat's drop, and taking the value
//!   a player's game sends again as it connects;
//! - a guest's own conversation bringing in nobody;
//! - a joined player leaving a locked conversation of a player's own, and
//!   kept in a party one;
//! - the reply taken shown before it runs: nothing of it running meanwhile,
//!   no other reply taken or pointed at, a story reply locking at once, its
//!   time KOTOR's reading time, its skip refused before `SKIP_AFTER_MS` and
//!   taken after (in an unskippable conversation too), a cut running it at
//!   once;
//! - a player alone, and a host who turned the option off, seeing a reply
//!   run at once;
//! - a computer's screen bringing in nobody, and its reply running at once.

use super::*;
use crate::script::dialogue::{Chosen, SKIP_AFTER_MS};

/// The chest among the objects (after the probes' placements), and where
/// it stands: within reach of the companion at `BUDDY_AT`.
const CHEST: u32 = 16;
const CHEST_AT: [f32; 2] = [-0.8, -1.5];

impl Run {
    /// Every seat joins the first seat's own conversations it stands near,
    /// and the host shows chosen replies (issues 636 and 637).
    fn share(&mut self, on: bool) {
        let mut state = self.world.script_state().clone();
        state.talk.quiet = match on {
            true => Default::default(),
            false => (0..self.seats).collect(),
        };
        self.world.set_script_state(state);
        self.world.set_show_replies(on);
    }

    /// Seat `seat` sends `action`, in one step.
    fn act(&mut self, seat: u8, action: Action) {
        let mut input = TickInput::default();
        input.act(Seat(seat), action);
        self.step_with(input);
    }

    /// The host uses the one who talks, the companion standing at
    /// `BUDDY_AT` and the second companion at `second`: the conversation
    /// it began.
    fn host_talks(&mut self, second: [f32; 2]) -> Result<Conversation, String> {
        let (buddy, buddy2) = (self.body(1)?, self.body(2)?);
        self.put(buddy, BUDDY_AT);
        self.put(buddy2, second);
        self.click(0, TALK_FROM, 0.0)?;
        self.talk()
            .cloned()
            .ok_or("the host's use began no conversation".into())
    }

    /// The reply conversation `id` shows, if it shows one.
    fn chosen(&self, id: u32) -> Option<Chosen> {
        self.conversation(id).and_then(|c| c.chosen)
    }

    /// How many of conversation `id`'s replies ran (`Line::Picked`).
    fn picked(&self, id: u32) -> usize {
        let talk = &self.world.script_report().talk;
        let ran = talk
            .iter()
            .filter(|line| matches!(line, Line::Picked { id: of, .. } if *of == id));
        ran.count()
    }

    /// Whether conversation `id` ended, cut short (`aborted`) or not.
    fn ended(&self, id: u32, aborted: bool) -> bool {
        said(
            self,
            |line| matches!(line, Line::Ended { id: of, aborted: cut, .. } if *of == id && *cut == aborted),
        )
    }
}

/// The shared conversations' cases (see the module's docs), on a run of
/// their own.
pub(super) fn shared() -> Result<Vec<Case>, String> {
    let chest = ObjectData {
        kind: ObjectKind::Placeable,
        container: true,
        ..creature("chest", [0.0; 3], 0.0, "")
    };
    let mut run = Run::on_ground(SEATS, Some(Seat(1)), |data| {
        let entry = data.entry.position;
        let at = [entry[0] + CHEST_AT[0], entry[1] + CHEST_AT[1], entry[2]];
        data.placements.push(Placement::Own(ObjectData {
            position: at,
            ..chest
        }));
    })?;
    run.follow(&[(BUDDY2, Some(Seat(2)))], Some(Seat(0)))?;
    run.silent.clear();
    run.share(true);
    run.step(&[]);
    let mut cases = joining(&mut run)?;
    cases.extend(leaving(&mut run)?);
    cases.extend(shown(&mut run)?);
    Ok(cases)
}

/// Who joins the host's own conversation as it begins (issue 636).
fn joining(run: &mut Run) -> Result<Vec<Case>, String> {
    let (buddy, buddy2) = (run.body(1)?, run.body(2)?);
    let id = |who: SimId| ObjectId(who.0);
    let joined = |c: &Conversation| c.joined.clone();
    // The companion near, the second companion out of range.
    let first = run.host_talks(FAR)?;
    let row = run.last.may_leave(Seat(1), &first);
    run.goodbye();
    // In a fight.
    run.in_combat(buddy, true);
    let fighting = run.host_talks(FAR)?;
    run.goodbye();
    run.in_combat(buddy, false);
    // A screen open on the guest's own machine.
    run.act(1, Action::Screen(true));
    let screen = run.host_talks(FAR)?;
    run.goodbye();
    run.act(1, Action::Screen(false));
    // A container open.
    run.put(buddy, BUDDY_AT);
    run.act(1, Action::Open(CHEST));
    let opened = object(&run.last, CHEST)
        .ok()
        .and_then(|chest| run.last.get(chest))
        .and_then(|chest| chest.object.as_ref())
        .is_some_and(|chest| chest.tenants != 0);
    let looting = run.host_talks(FAR)?;
    run.goodbye();
    run.act(1, Action::Close(CHEST));
    // The option off; kept through a save and a drop; sent again on.
    run.act(1, Action::JoinTalks(false));
    let quiet = run.host_talks(FAR)?;
    run.goodbye();
    *run = run.reloaded()?;
    run.drop_seat(1);
    run.rejoin(1, BUDDY)?;
    let kept = run.host_talks(FAR)?;
    run.goodbye();
    run.act(1, Action::JoinTalks(true));
    let again = run.host_talks(BUDDY2_AT)?;
    run.goodbye();
    // A guest's own: neither the host nor the second guest near joins.
    let host = run.body(0)?;
    run.put(host, HOST_ASIDE);
    run.put(buddy2, BUDDY2_AT);
    run.click(1, TALK_FROM, 0.0)?;
    let guests = run.talk().cloned();
    run.goodbye();
    let only = |c: &Conversation, bodies: &[SimId]| {
        joined(c) == bodies.iter().map(|b| id(*b)).collect::<Vec<_>>()
    };
    Ok(vec![
        (
            "shared_near_player_joins",
            check(
                only(&first, &[buddy]) && row == Some(true),
                format!("{:?}, the guest's Leave row {row:?}", joined(&first)),
            ),
        ),
        (
            "shared_far_player_not_joined",
            check(
                !first.joined.contains(&id(buddy2)),
                format!("{:?}", joined(&first)),
            ),
        ),
        (
            "shared_fighting_not_joined",
            check(only(&fighting, &[]), format!("{:?}", joined(&fighting))),
        ),
        (
            "shared_screen_open_not_joined",
            check(only(&screen, &[]), format!("{:?}", joined(&screen))),
        ),
        (
            "shared_container_open_not_joined",
            check(
                opened && only(&looting, &[]),
                format!("the chest open {opened}, {:?}", joined(&looting)),
            ),
        ),
        (
            "shared_option_off_not_joined",
            check(only(&quiet, &[]), format!("{:?}", joined(&quiet))),
        ),
        (
            "shared_option_kept_and_sent_again",
            check(
                only(&kept, &[]) && only(&again, &[buddy, buddy2]),
                format!(
                    "after a save and a drop {:?}, then sent on again {:?}",
                    joined(&kept),
                    joined(&again)
                ),
            ),
        ),
        (
            "shared_guest_talk_brings_nobody",
            check(
                guests
                    .as_ref()
                    .is_some_and(|c| c.listener == id(buddy) && c.joined.is_empty()),
                format!("{guests:?}"),
            ),
        ),
    ])
}

/// Leaving with the others in (issue 636): a joined player steps out of a
/// player's own conversation locked to the story, not out of a party one.
fn leaving(run: &mut Run) -> Result<Vec<Case>, String> {
    let (buddy, buddy2) = (run.body(1)?, run.body(2)?);
    run.put(buddy, BUDDY_AT);
    run.put(buddy2, FAR);
    let locked = run.start(0, TALKER, "probe_global")?;
    let before = run.conversation(locked);
    let row = before.as_ref().and_then(|c| run.last.may_leave(Seat(1), c));
    let (up, refused) = run.leave(1, locked, "locked");
    let after = run.conversation(locked);
    let host_refused = run.leave(0, locked, "locked");
    run.goodbye();
    run.cutscene()?;
    let scene = run
        .talk()
        .cloned()
        .ok_or("the script began no conversation")?;
    let kept = run.leave(1, scene.id, "locked");
    run.goodbye();
    Ok(vec![
        (
            "shared_joined_leaves_locked_own",
            check(
                before
                    .as_ref()
                    .is_some_and(|c| c.locked && c.joined == [ObjectId(buddy.0)])
                    && row == Some(true)
                    && up
                    && refused == 0
                    && after.as_ref().is_some_and(|c| c.joined.is_empty())
                    && host_refused == (true, 1),
                format!(
                    "{before:?}, the guest's Leave row {row:?}; after its leave (up, refused) ({up}, {refused}) {after:?}; the host's leave {host_refused:?}"
                ),
            ),
        ),
        (
            "shared_party_talk_keeps_players",
            check(
                scene.party && kept == (true, 1),
                format!("{scene:?}, the guest's leave (up, refused) {kept:?}"),
            ),
        ),
    ])
}

/// The reply taken, shown before it runs (issue 637).
fn shown(run: &mut Run) -> Result<Vec<Case>, String> {
    let host = run.body(0)?;
    // "Goodbye." taken by the host, "Again." by the guest in the same step
    // (the host's, first in seat order, wins), then the guest's point.
    let talk = run.host_talks(FAR)?.id;
    run.answers(&[(0, Pick::Reply(GOODBYE)), (1, Pick::Reply(AGAIN))]);
    let taken = run.chosen(talk);
    let waiting = (run.picked(talk), run.ended(talk, false));
    run.answers_now(&[(1, Pick::Point(AGAIN))]);
    let marks = run.conversation(talk).map(|c| c.marks);
    let lasted = steps(taken.map_or(0, |chosen| chosen.until_ms - chosen.from_ms));
    run.idle(lasted);
    let ran = (run.picked(talk), run.ended(talk, false));
    let reading = taken.map(|chosen| chosen.until_ms - chosen.from_ms);
    // A story reply locks at once.
    let story = run.host_talks(FAR)?.id;
    run.answers(&[(0, Pick::Reply(JOIN))]);
    let locked = run
        .conversation(story)
        .map(|c| (c.locked, c.chosen.is_some()));
    run.idle(steps(rules::dialogue::coop::SHOWN_REPLY_MOST_MS) + 1);
    run.goodbye();
    // A skip too soon is refused; one after `SKIP_AFTER_MS` runs it.
    let skipped = run.host_talks(FAR)?.id;
    run.answers(&[(0, Pick::Reply(AGAIN))]);
    run.answers_now(&[(1, Pick::Skip)]);
    let soon = run.chosen(skipped).is_some();
    run.idle(steps(SKIP_AFTER_MS));
    run.answers_now(&[(1, Pick::Skip)]);
    let later = (run.chosen(skipped).is_none(), run.picked(skipped));
    run.goodbye();
    // A cutscene, whose lines cannot be skipped: its shown reply can.
    run.click(0, CUT_FROM, 180.0)?;
    let cut = run.talk().cloned().ok_or("the cutscene did not begin")?;
    run.answers(&[(0, Pick::Reply(AGAIN))]);
    let shown_in_cut = run.chosen(cut.id).is_some();
    run.idle(steps(SKIP_AFTER_MS));
    run.answers_now(&[(1, Pick::Skip)]);
    let skipped_cut = run.chosen(cut.id).is_none() && run.picked(cut.id) == 1;
    run.goodbye();
    // The host's Leave during the shown reply: it runs, then the cut.
    let left = run.host_talks(FAR)?.id;
    run.answers(&[(0, Pick::Reply(AGAIN))]);
    let before_cut = run.chosen(left).is_some();
    run.answers_now(&[(0, Pick::Leave)]);
    let cut_ran = (run.picked(left), run.ended(left, true));
    run.goodbye();
    // The guest alone in its own conversation sees its reply run at once.
    // So does the host with the guest joined, once the host turns "Show
    // chosen replies" off.
    run.put(host, HOST_ASIDE);
    run.click(1, TALK_FROM, 0.0)?;
    let alone = run
        .talk()
        .map(|c| c.id)
        .ok_or("the guest's use began nothing")?;
    run.answers(&[(1, Pick::Reply(AGAIN))]);
    let alone_ran = (run.chosen(alone).is_none(), run.picked(alone));
    run.goodbye();
    run.world.set_show_replies(false);
    let hidden = run.host_talks(FAR)?;
    run.answers(&[(0, Pick::Reply(AGAIN))]);
    let hidden_ran = (
        hidden.joined.len(),
        run.chosen(hidden.id).is_none(),
        run.picked(hidden.id),
    );
    run.goodbye();
    run.world.set_show_replies(true);
    // A computer the host uses with the guest near: one player uses a
    // computer, so the guest is not brought in and the reply runs at once.
    let buddy = run.body(1)?;
    run.put(buddy, BUDDY_AT);
    let screen = run.start(0, TALKER, "probe_screen")?;
    let screen_joined = run.conversation(screen).map(|c| c.joined.len());
    run.answers(&[(0, Pick::Reply(AGAIN))]);
    let screen_ran = (run.chosen(screen).is_none(), run.picked(screen));
    run.goodbye();
    let read_ms = |text: &str| {
        (text.len() as u64 * rules::dialogue::lines::READ_TENTHS_MS_PER_CHAR / 10).clamp(
            rules::dialogue::lines::LEAST_LINE_MS,
            rules::dialogue::coop::SHOWN_REPLY_MOST_MS,
        )
    };
    Ok(vec![
        (
            "shared_reply_shown",
            check(
                taken.is_some_and(|chosen| chosen.nth == GOODBYE && chosen.by == ObjectId(host.0))
                    && reading == Some(read_ms("Goodbye.")),
                format!("{taken:?}, shown for {reading:?} ms"),
            ),
        ),
        (
            "shared_reply_waits",
            check(
                waiting == (0, false) && marks.as_ref().is_some_and(BTreeMap::is_empty),
                format!(
                    "while shown: replies run, ended {waiting:?}; the guest's later point left marks {marks:?}"
                ),
            ),
        ),
        (
            "shared_reply_runs_after",
            check(ran == (1, true), format!("replies run, ended {ran:?}")),
        ),
        (
            "shared_story_reply_locks_at_once",
            check(
                locked == Some((true, true)),
                format!("locked, shown {locked:?}"),
            ),
        ),
        (
            "shared_skip_after_skip_delay",
            check(
                soon && later == (true, 1),
                format!(
                    "shown after a skip too soon {soon}; after the delay ran, replies run {later:?}"
                ),
            ),
        ),
        (
            "shared_skip_in_unskippable",
            check(
                cut.party && shown_in_cut && skipped_cut,
                format!("{cut:?}: shown {shown_in_cut}, skipped and run {skipped_cut}"),
            ),
        ),
        (
            "shared_cut_runs_reply",
            check(
                before_cut && cut_ran == (1, true),
                format!(
                    "shown {before_cut}; after the host's Leave replies run, cut short {cut_ran:?}"
                ),
            ),
        ),
        (
            "shared_alone_runs_at_once",
            check(alone_ran == (true, 1), format!("{alone_ran:?}")),
        ),
        (
            "shared_hidden_runs_at_once",
            check(
                hidden_ran == (1, true, 1),
                format!("joined, run at once, replies run {hidden_ran:?}"),
            ),
        ),
        (
            "shared_computer_not_shared",
            check(
                screen_joined == Some(0) && screen_ran == (true, 1),
                format!("joined {screen_joined:?}; run at once, replies run {screen_ran:?}"),
            ),
        ),
    ])
}
