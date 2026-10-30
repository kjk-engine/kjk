//! The step's dialogue pass (`advance`) and what the engine asks of
//! conversations.

use super::*;

/// Who runs their end-of-conversation scripts as a conversation ends
/// normally (`Step::heard_end`).
#[derive(Clone, Copy)]
pub(super) enum Heard {
    /// Every creature and placeable of the area.
    Area,
    /// Its owner only: a one-liner's.
    Owner,
}

/// The step's dialogue pass: what the world did to those talking, starts
/// asked for, the players' picks, lines that have played or waited enough,
/// barks that are over, and a module left.
pub(in crate::script) fn advance(step: &mut Step) {
    let now = step.view.now_ms;
    // Damage first, then the bodies seats let go (R7, R12).
    for body in std::mem::take(&mut step.state.talk.hurt) {
        step.hurt(body);
    }
    for (body, why) in std::mem::take(&mut step.state.talk.released) {
        step.released(body, why);
    }
    for start in std::mem::take(&mut step.state.talk.asked) {
        step.begin(start);
    }
    let view = step.view;
    // The host's answers to requests (R5) go before the picks, so the host
    // answers one it accepts in the same step.
    for &(_, id, accept) in view.requests {
        step.request(id, accept);
    }
    // Pointing marks the conversation each seat answers (I30 P6). Marks go
    // first, so one sent with the answer that moves the line goes with
    // that line.
    for (&seat, answer) in view.picks {
        if let Pick::Point(nth) = answer.pick {
            step.mark(seat, answer.by, nth);
        }
    }
    let (pcs, lent) = (view.pcs(&step.state.party), step.lent());
    let pc = |id: ObjectId| party_pc(&pcs, lent, id);
    // The first answer wins: in seat order, one a conversation a step. A
    // leave is each seat's own, whoever answered.
    let mut answered = Vec::new();
    for (&seat, answer) in view.picks {
        let Some((at, own)) = step.answers(answer.by) else {
            continue;
        };
        if answer.pick == Pick::Leave {
            // Spent on the seat's watching, it leaves no conversation (R9a).
            if !step.left_watching.contains(&seat) {
                step.leave(seat, answer.by, (at, own));
            }
            continue;
        }
        let c = &step.state.talk.conversations[at];
        let (id, auto, waiting) = (c.id, c.auto, c.until_ms == u64::MAX);
        if answered.contains(&id) {
            continue;
        }
        let library = step.library;
        let skippable = now >= c.shown_ms + SKIP_AFTER_MS
            && library
                .dialogue(&c.dialog)
                .is_some_and(|dlg| skippable(dlg, c.entry));
        // Its listener answers as soon as replies are offered (the line
        // played, `offer_replies`); another once the replies are up
        // (one sent for the line before, arriving late, takes nothing).
        let may =
            own || (answer.by != ObjectId::INVALID && waiting && now >= c.shown_ms + SKIP_AFTER_MS);
        match answer.pick {
            Pick::Reply(nth) => {
                let Some(&reply) = c.replies.get(usize::from(nth)) else {
                    continue;
                };
                // A story reply is a PC's to take (I30 P6). Picked where no
                // PC stands, it asks the host in (R5); either way it marks.
                let moves = || {
                    library
                        .dialogue(&c.dialog)
                        .is_some_and(|dlg| story(library, dlg, reply))
                };
                if !auto && may {
                    if pc(answer.by) || !moves() {
                        answered.push(id);
                        step.state.talk.conversations[at].asker = answer.by;
                        step.pick(id, usize::from(nth));
                        continue;
                    }
                    let stands = std::iter::once(&c.listener).chain(&c.joined);
                    if !stands.copied().any(pc) {
                        step.ask(at, answer.by);
                    }
                }
                if may || waiting {
                    step.mark(seat, answer.by, nth);
                }
            }
            Pick::Skip if !waiting && skippable && answer.by != ObjectId::INVALID => {
                answered.push(id);
                step.played(id);
            }
            _ => {}
        }
    }
    let due: Vec<u32> = step
        .state
        .talk
        .conversations
        .iter()
        // A line lasts a step at least: what its script queued (a pause
        // while someone walks up) acts first.
        .filter(|c| !c.on_hold() && c.until_ms != u64::MAX && now >= c.until_ms && now > c.shown_ms)
        .map(|c| c.id)
        .collect();
    for id in due {
        step.played(id);
    }
    // Someone gone ends theirs, cut short: its owner or its listener (R12),
    // not the host's character as the PC of a guest's own; one player alone,
    // its owner or its PC, as ever. One made this step is not gone: it is
    // in the world from the step's end (TSL's prologue switches the player
    // to T3-M4, made for it, while its opening conversation plays). Nor is
    // a party member knocked out: it gets up again (`Thing::dead_for_good`;
    // Atton, poisoned in Peragus' hangar bay, falls as his escape's scene
    // begins). An owner dead but not taken away talks on: its conversation
    // is the object's, which stays (TSL's Visquis, killed by his own
    // conversation's script as Mira watches the monitor, `a_305end_setup0`,
    // and the scene goes on) [verify in the exe].
    let made: Vec<ObjectId> = step.out.made.iter().map(|made| made.id).collect();
    let gone: Vec<u32> = step
        .state
        .talk
        .conversations
        .iter()
        .filter(|c| {
            let them = if view.together() {
                [c.owner, c.listener]
            } else {
                [c.owner, c.pc]
            };
            them.iter().any(|id| match view.thing(*id) {
                Some(_) if *id == c.owner && c.owner != c.listener && c.owner != c.pc => false,
                Some(thing) => thing.dead_for_good(),
                None => !made.contains(id),
            })
        })
        .map(|c| c.id)
        .collect();
    for id in gone {
        step.end(id, true);
    }
    // What waits on someone asks again (R4, R12).
    let waiting: Vec<u32> = step
        .state
        .talk
        .conversations
        .iter()
        .filter(|c| c.waits_for != ObjectId::INVALID)
        .map(|c| c.id)
        .collect();
    for id in waiting {
        step.waited(id);
    }
    step.state.talk.barks.retain(|bark| bark.until_ms > now);
    step.leaving_module();
}

impl Engine<'_> {
    /// The conversation routines (`docs/DIALOGUE.md`).
    pub(in crate::script) fn talk(
        &mut self,
        bound: Bound,
        caller: ObjectId,
        args: &[Value],
    ) -> Value {
        use Bound::*;
        let view = self.view;
        match bound {
            // Refused while a conversation holds the world, as both games'
            // executables refuse it (their dialog state set), its own
            // nodes' scripts too: a locked door's line as a scene walks
            // someone into it is never said.
            ActionStartConversation if self.state.talk.holds_world(&view.seat_bodies()) => {
                Value::Int(0)
            }
            ActionStartConversation => {
                let with = object(args, 0);
                // Only a creature walks to whom it talks to: a door's or a
                // placeable's walk fails at once and its conversation starts
                // where it stands (a locked door's line as it is tried, not
                // once the player happens by).
                let walks = view.thing(caller).is_none_or(|thing| {
                    !matches!(
                        thing.kind,
                        Some(
                            crate::module::ObjectKind::Door | crate::module::ObjectKind::Placeable
                        )
                    )
                });
                if int(args, 4) == 0 && with != caller && walks {
                    self.queue(
                        caller,
                        Action::Move {
                            to: crate::script::actions::Target::Object(with),
                            run: true,
                            range: crate::script::engine::CONVERSATION_RANGE,
                            force_ms: None,
                        },
                    );
                }
                self.queue(
                    caller,
                    Action::Converse {
                        with,
                        dialog: key(text(args, 1)),
                        asker: self.event.asker,
                    },
                )
            }
            // Refused while a conversation holds the world, as
            // `ActionStartConversation` is (both games' executables; TSL's
            // lets the dialog action's own `OnDialog` run begin it [verify
            // K1], here none: no click starts one while a scene holds).
            BeginConversation if self.state.talk.holds_world(&view.seat_bodies()) => {
                if let Some(dialog) = view.dialog_or_own(text(args, 0), caller) {
                    self.report.talk.push(Line::Refused {
                        ms: view.now_ms,
                        dialog,
                        why: "already talking",
                    });
                }
                truth(false)
            }
            BeginConversation => {
                let dialog = view.dialog_or_own(text(args, 0), caller);
                let Some(dialog) = dialog else {
                    return truth(false);
                };
                let speaker = self
                    .state
                    .last
                    .get(&(caller.0, last::SPEAKER))
                    .map(|id| ObjectId(*id));
                let named = Some(object(args, 1))
                    .filter(|id| *id != ObjectId::INVALID)
                    .or(speaker)
                    .unwrap_or_else(|| view.first_pc(&self.state.party));
                let (pc, asker) = view.speakers(&self.state.party, named, self.event.asker);
                self.state.talk.asked.push(Start {
                    owner: caller,
                    pc,
                    asker,
                    dialog,
                    bark: false,
                });
                truth(true)
            }
            SpeakOneLinerConversation => {
                let dialog = view.dialog_or_own(text(args, 0), caller);
                if let Some(dialog) = dialog {
                    self.state.talk.asked.push(Start {
                        owner: caller,
                        pc: view.first_pc(&self.state.party),
                        asker: ObjectId::INVALID,
                        dialog,
                        bark: true,
                    });
                }
                Value::Int(0)
            }
            // Dropped while a conversation holds the world, as both games'
            // executables drop it (their dialog state set).
            BarkString => {
                if let Ok(strref) = u32::try_from(int(args, 1))
                    && !self.state.talk.holds_world(&view.seat_bodies())
                {
                    bark(
                        self.library,
                        self.state,
                        self.report,
                        view.now_ms,
                        (object(args, 0), strref),
                    );
                }
                Value::Int(0)
            }
            GetIsInConversation => {
                let host = view.host(&self.state.party);
                truth(self.state.talk.with(object(args, 0), host).is_some())
            }
            GetIsConversationActive => truth(!self.state.talk.conversations.is_empty()),
            GetPCSpeaker => {
                let (talk, host) = (&self.state.talk, view.host(&self.state.party));
                let pc = talk
                    .running
                    .or_else(|| talk.with(caller, host).map(|c| c.pc));
                Value::Object(pc.unwrap_or_else(|| view.first_pc(&self.state.party)))
            }
            // A cut to a placed camera now, for the conversation whose script
            // runs it (`running`), else the one the caller is in, else
            // the first under way (scenes run it from objects that take no
            // part: the Endar Spire's explosions); outside a conversation it
            // does nothing.
            SetDialogPlaceableCamera => {
                let any = (!self.state.talk.conversations.is_empty()).then_some(0);
                if let Some(at) = self.acted_on(caller).or(any) {
                    self.state.talk.conversations[at].camera = int(args, 0);
                }
                Value::Int(0)
            }
            // An object put on (true) or taken off a lock list (S2) of the
            // conversation whose script runs it, else of the one the
            // caller takes part in; outside a conversation it does nothing.
            SetLockOrientationInDialog | SetLockHeadFollowInDialog => {
                let (who, on) = (object(args, 0), int(args, 1) != 0);
                if let Some(at) = self.acted_on(caller) {
                    let c = &mut self.state.talk.conversations[at];
                    let list = if bound == SetLockOrientationInDialog {
                        &mut c.lock_turn
                    } else {
                        &mut c.lock_head
                    };
                    list.retain(|id| *id != who);
                    if on {
                        list.push(who);
                    }
                }
                Value::Int(0)
            }
            // The pause holds the conversation at once; the resume waits its
            // turn behind the caller's actions (S5), so a pause, some acting
            // and a resume hold the line through the acting. The pause keeps
            // its place in the queue too: a clear before it reaches the
            // front drops it (`Engine::clear_queue`).
            ActionPauseConversation => {
                let talk = running(&self.state.talk, &self.event);
                let host = view.host(&self.state.party);
                if let Some(at) = self.state.talk.held_by(caller, talk, host) {
                    self.state.talk.conversations[at].paused = true;
                }
                self.queue(caller, Action::Pause { talk })
            }
            ActionResumeConversation => {
                let talk = running(&self.state.talk, &self.event);
                self.queue(caller, Action::Hold { talk })
            }
            SetCustomToken => {
                let (number, value) = (int(args, 0), text(args, 1).to_string());
                self.report.talk.push(Line::Token {
                    ms: self.view.now_ms,
                    number,
                    value: value.clone(),
                });
                self.state.talk.tokens.insert(number, value);
                Value::Int(0)
            }
            GetScriptParameter => {
                let n = int(args, 0)
                    .checked_sub(1)
                    .and_then(|n| usize::try_from(n).ok());
                let params = self.event.params.as_ref();
                Value::Int(n.and_then(|n| params?.ints.get(n).copied()).unwrap_or(0))
            }
            GetScriptStringParameter => crate::script::engine::string(
                self.event.params.as_ref().map_or_else(String::new, |p| {
                    p.string.iter().map(|&b| char::from(b)).collect()
                }),
            ),
            _ => unreachable!("not a conversation routine: {bound:?}"),
        }
    }

    /// The conversation a routine run by `caller` acts on: the one whose
    /// script runs it (`running`), else the one `caller` takes part in.
    pub(super) fn acted_on(&self, caller: ObjectId) -> Option<usize> {
        let (talk, host) = (&self.state.talk, self.view.host(&self.state.party));
        running(talk, &self.event)
            .and_then(|id| talk.index(id))
            .or_else(|| {
                talk.conversations
                    .iter()
                    .position(|c| c.takes_part(caller, host))
            })
    }
}

impl Step<'_> {
    /// `Action::Hold` at the front of `who`'s queue: the conversation whose
    /// script queued it (`talk`) goes on, where `who` takes part in it or is
    /// its PC (a guest's line resuming `GetPCSpeaker`'s); else the one `who`
    /// takes part in.
    pub(in crate::script) fn resume(&mut self, who: ObjectId, talk: Option<u32>) {
        let host = self.host();
        if let Some(at) = self.state.talk.held_by(who, talk, host) {
            self.state.talk.conversations[at].paused = false;
        }
    }

    /// `Action::Converse` at the front of `who`'s queue: ask to start a
    /// conversation, its owner by reone's rule (the other party when a
    /// player asks, else the asker), its DLG the one named or the owner's.
    /// With none named, a non-player conversed with that has a DLG of its
    /// own owns it with that DLG, as nwscript.nss says starting one "will
    /// cause their OnDialog event to fire": the one conversed with speaks
    /// (an invisible placeable a scene speaks through: Taris's raid, the
    /// swoop pits' Brejik). reone keeps the asker's DLG [verify in the
    /// game].
    pub(in crate::script) fn converse(
        &mut self,
        who: ObjectId,
        with: ObjectId,
        dialog: &str,
        asker: Option<ObjectId>,
    ) {
        let view = self.view;
        // The players' side: their characters, and the companion the host
        // leads through (`GetPartyMemberByIndex(0)`: a console's
        // `a_compdlg` talks to it with the console's own dialog, TSL's
        // 3C-FD at the Ebon Hawk's security console).
        let mut pcs = view.pcs(&self.state.party);
        pcs.extend(self.host());
        let (owner, pc) = if pcs.contains(&who) && with != ObjectId::INVALID {
            (with, who)
        } else if pcs.contains(&with) {
            (who, with)
        } else {
            (who, view.first_pc(&self.state.party))
        };
        let through = (dialog.is_empty() && with != ObjectId::INVALID && !pcs.contains(&with))
            .then(|| view.conversation(with).map(str::to_string))
            .flatten();
        let (owner, dialog) = match through {
            Some(dialog) => (with, Some(dialog)),
            None => (
                owner,
                view.dialog_or_own(dialog, owner)
                    .or_else(|| view.conversation(who).map(str::to_string)),
            ),
        };
        let (pc, asker) = view.speakers(&self.state.party, pc, asker);
        if let Some(dialog) = dialog {
            self.state.talk.asked.push(Start {
                owner,
                pc,
                asker,
                dialog,
                bark: false,
            });
        }
    }
}
