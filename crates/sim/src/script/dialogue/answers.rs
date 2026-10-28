//! The seats' answers: picks, marks, asks, leaving, the host's requests.

use super::*;

impl Step<'_> {
    /// The player takes reply `nth` of those offered.
    pub(super) fn pick(&mut self, id: u32, nth: usize) {
        let now = self.view.now_ms;
        let library = self.library;
        let Some(at) = self.state.talk.index(id) else {
            return;
        };
        let (owner, pc, dialog, reply) = {
            let c = &self.state.talk.conversations[at];
            let Some(reply) = c.replies.get(nth) else {
                return;
            };
            (c.owner, c.pc, c.dialog.clone(), *reply)
        };
        let Some(dlg) = library.dialogue(&dialog) else {
            return;
        };
        self.state.talk.conversations[at].locked |= story(library, dlg, reply);
        self.line(Line::Picked {
            ms: now,
            id,
            dialog: dialog.clone(),
            reply,
        });
        let node = &dlg.replies[reply as usize];
        // A reply said is a line of the player's (S2); a silent one, taken
        // as an entry's line ends, is none.
        if !node.is_silent() {
            let c = &mut self.state.talk.conversations[at];
            c.angle = c.reply_angle(dlg, reply);
            let c = &self.state.talk.conversations[at];
            let player = self.player(c);
            let listener = self.listener_for(node, player, c, player);
            self.says(at, player, listener);
        }
        self.progress(node);
        self.node_fade(&node.fade);
        for call in [&node.script, &node.script2] {
            self.call(call, id, owner, pc);
        }
        let next = node
            .links
            .iter()
            .filter(|link| (link.index as usize) < dlg.entries.len())
            .find(|link| {
                self.speaker_here(dlg, link.index, owner) && self.holds(link, id, owner, pc)
            })
            .map(|link| link.index);
        match next {
            Some(entry) => {
                let replies = self.offer(dlg, id, entry, owner, pc);
                self.show(id, entry, replies);
            }
            None => self.end(id, false),
        }
    }

    /// The line has played: take the silent reply, end, or wait. The
    /// replies offered start a line (S2): the player's, to the one who
    /// spoke (`listener_now`), the pair before it kept, filmed by the first
    /// reply's angle (S6).
    pub(super) fn played(&mut self, id: u32) {
        let Some(at) = self.state.talk.index(id) else {
            return;
        };
        let c = &mut self.state.talk.conversations[at];
        c.until_ms = u64::MAX;
        if c.auto {
            self.pick(id, 0);
        } else if c.replies.is_empty() {
            self.end(id, false);
        } else {
            let spoke = c.speaker;
            c.listener_now = spoke;
            if let Some(dlg) = self.library.dialogue(&c.dialog) {
                c.angle = c.reply_angle(dlg, c.replies[0]);
            }
            let player = self.player(&self.state.talk.conversations[at]);
            self.face_pair(at, player, spoke);
        }
    }

    /// The conversation an answer by `by` (a seat's body, `INVALID` for a
    /// watching seat) is for, and whether `by` is its listener (the host's
    /// two bodies one): the one `by` stands in (R3), else the one holding
    /// every seat (R1). One on hold (`Conversation::on_hold`) takes no
    /// answer and no mark.
    pub(super) fn answers(&self, by: ObjectId) -> Option<(usize, bool)> {
        let (talk, host) = (&self.state.talk.conversations, self.host());
        let listens =
            |c: &Conversation| c.listener == by || host.contains(&by) && host.contains(&c.listener);
        let stands = talk
            .iter()
            .position(|c| by != ObjectId::INVALID && c.holds(by));
        let (at, own) = match stands {
            Some(at) => (at, listens(&talk[at])),
            None => {
                let at = talk.iter().position(|c| c.party)?;
                (at, by != ObjectId::INVALID && listens(&talk[at]))
            }
        };
        (!talk[at].on_hold()).then_some((at, own))
    }

    /// Seat `seat`, its body `by`, points at reply `nth` of the
    /// conversation it answers (`answers`, I30 P6): a marker everyone in it
    /// sees, never an answer. A place past those offered, or a line whose
    /// one reply is silent, marks nothing.
    pub(super) fn mark(&mut self, seat: u8, by: ObjectId, nth: u8) {
        let Some((at, _)) = self.answers(by) else {
            return;
        };
        let c = &mut self.state.talk.conversations[at];
        let Some(&reply) = c.replies.get(usize::from(nth)) else {
            return;
        };
        if c.auto || c.marks.insert(seat, nth) == Some(nth) {
            return;
        }
        let line = Line::Marked {
            ms: self.view.now_ms,
            id: c.id,
            dialog: c.dialog.clone(),
            seat,
            reply,
        };
        self.line(line);
    }

    /// `by` picked a story reply of the conversation at `at`, where no PC
    /// stands: it asks the host in (R5), unless another asks already.
    pub(super) fn ask(&mut self, at: usize, by: ObjectId) {
        let c = &mut self.state.talk.conversations[at];
        if c.asking != ObjectId::INVALID {
            return;
        }
        c.asking = by;
        let line = Line::Asked {
            ms: self.view.now_ms,
            id: c.id,
            by,
        };
        self.line(line);
    }

    /// Seat `seat`, its body `by`, leaves the conversation at `at` it
    /// answers (`answers`; `own` when `by` is its listener), R6. Unlocked,
    /// its listener ends it cut short (`EndConverAbort`), and one joined to
    /// a player's own steps out, its mark and its request going with it. A
    /// locked one, or a party one left by anyone but its listener, is
    /// refused: a party one holds every seat anyway.
    pub(super) fn leave(&mut self, seat: u8, by: ObjectId, (at, own): (usize, bool)) {
        let c = &mut self.state.talk.conversations[at];
        let refused = if c.locked {
            Some("locked")
        } else if c.party && !own {
            Some("the host's")
        } else {
            None
        };
        if let Some(why) = refused {
            let line = Line::Refused {
                ms: self.view.now_ms,
                dialog: c.dialog.clone(),
                why,
            };
            self.line(line);
        } else if own {
            let id = c.id;
            self.end(id, true);
        } else {
            c.step_out(by, Some(seat));
        }
    }

    /// The host's two bodies (`View::host`).
    pub(super) fn host(&self) -> [ObjectId; 2] {
        self.view.host(&self.state.party)
    }

    /// The body a conversation's lines are said to where no other listens
    /// (`listener_for`), which says its replies and its `AnimList` names
    /// `player` (R2): its listener where that is a guest seat's body, else
    /// its PC, so one player leading through a companion is faced as before
    /// (the dialog camera frames the same, `play_view::talk_camera`).
    pub(super) fn player(&self, c: &Conversation) -> ObjectId {
        let guest = self
            .view
            .seat_of(c.listener)
            .is_some_and(|seat| seat != crate::world::LEADER_SEAT.0);
        if guest { c.listener } else { c.pc }
    }

    /// The host's answer to the request to join conversation `id` (R5).
    /// While the party is in combat, or the host stands in another locked
    /// conversation, the request waits and the answer does nothing; so it
    /// does for one gone or not asking. Accepted, it becomes the host's
    /// (`hand_to_host`), the listener before joined. Refused, it is cut short,
    /// locked or not.
    pub(super) fn request(&mut self, id: u32, accept: bool) {
        let talk = &self.state.talk;
        let asking = talk
            .index(id)
            .is_some_and(|at| talk.conversations[at].asking != ObjectId::INVALID);
        if !asking || self.host_locked_elsewhere(id) || self.view.in_combat {
            return;
        }
        if !accept {
            self.end(id, true);
            return;
        }
        if let Some(asked) = self.hand_to_host(id, "the host joined another")
            && let Some(at) = self.state.talk.index(id)
        {
            self.state.talk.conversations[at].joined.push(asked);
        }
    }

    /// Conversation `id` becomes the party's, for the host to answer (R5,
    /// R12): the host's own conversations cut short (`why`; one it joined,
    /// it steps out of), the host's character its listener, every seat
    /// gathered to its owner but those in another player's own. The
    /// listener before; `None` when an end script ended it too.
    pub(super) fn hand_to_host(&mut self, id: u32, why: &'static str) -> Option<ObjectId> {
        let host = self.host();
        let own: Vec<u32> = self
            .state
            .talk
            .conversations
            .iter()
            .filter(|c| c.id != id && host.contains(&c.listener))
            .map(|c| c.id)
            .collect();
        self.cut(own, why);
        let talk = &mut self.state.talk;
        let at = talk.index(id)?;
        for c in &mut talk.conversations {
            c.joined.retain(|body| !host.contains(body));
        }
        let c = &mut talk.conversations[at];
        let before = std::mem::replace(&mut c.listener, host[0]);
        c.party = true;
        c.asking = ObjectId::INVALID;
        self.out.changes.push(crate::script::Change::Gather {
            around: c.owner,
            who: crate::script::Gathered::Everyone,
        });
        self.close_stores();
        Some(before)
    }
}
