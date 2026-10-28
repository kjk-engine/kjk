//! Conversations cut short, interjected, interrupted, handed over, waited
//! on, and left with the module.

use super::*;

impl Step<'_> {
    /// A party conversation began (R7): every other one a seat's body
    /// stands in is cut short, locked or not.
    pub(super) fn cut_short(&mut self) {
        let view = self.view;
        let held: Vec<u32> = self
            .state
            .talk
            .conversations
            .iter()
            .filter(|c| view.seats.iter().any(|(_, body)| c.holds(*body)))
            .map(|c| c.id)
            .collect();
        self.cut(held, "a cutscene began");
    }

    /// Each of conversations `ids` still under way is cut short
    /// (`EndConverAbort`), noted with `why`.
    pub(super) fn cut(&mut self, ids: Vec<u32>, why: &'static str) {
        for id in ids {
            if self.state.talk.index(id).is_none() {
                continue;
            }
            self.line(Line::CutShort {
                ms: self.view.now_ms,
                id,
                why,
            });
            self.end(id, true);
        }
    }

    /// Whether conversation `id`'s next line, said by `speaker`, plays now
    /// (R4). A seat's body standing outside a player's own or a party
    /// conversation says it where it stands, never moved: it joins the
    /// conversation (a party one holds it already), first cutting its own
    /// unlocked one short, or stepping out of another player's it joined
    /// (R6). While the one it stands in is locked, or the party's, the
    /// line waits on it (`Conversation::waits_for`) until it is free, for
    /// `INTERJECTION_WAIT_MS` at most; then it plays without that body.
    pub(super) fn interjection(&mut self, id: u32, speaker: ObjectId) -> bool {
        let view = self.view;
        let talk = &self.state.talk;
        let Some(at) = talk.index(id) else {
            return false;
        };
        let c = &talk.conversations[at];
        let ours = c.party || view.seat_of(c.listener).is_some();
        if !ours || view.seat_of(speaker).is_none() || c.holds(speaker) {
            return true;
        }
        let elsewhere = talk
            .conversations
            .iter()
            .find(|other| other.id != id && other.holds(speaker))
            .map(|other| {
                (
                    other.id,
                    other.locked || other.party,
                    other.listener == speaker,
                )
            });
        match elsewhere {
            Some((_, true, _)) => {
                let now = view.now_ms;
                let c = &mut self.state.talk.conversations[at];
                if c.waits_for != speaker {
                    c.waits_for = speaker;
                    c.shown_ms = now;
                }
                return now >= c.shown_ms + INTERJECTION_WAIT_MS;
            }
            Some((other, false, true)) => self.cut(vec![other], "an interjection"),
            Some((other, false, false)) => {
                if let Some(at) = self.state.talk.index(other) {
                    let seat = view.seat_of(speaker);
                    self.state.talk.conversations[at].step_out(speaker, seat);
                }
            }
            None => {}
        }
        let pc = party_pc(&view.pcs(&self.state.party), self.lent(), speaker);
        let talk = &mut self.state.talk;
        let Some(at) = talk.index(id) else {
            return false;
        };
        let c = &mut talk.conversations[at];
        if !c.party {
            c.take_in(speaker, pc);
        }
        true
    }

    /// `body` took damage (R7) while more than one seat plays a body: the
    /// player's own conversation it listens in (a seat's body) is cut
    /// short, locked or not, unless its owner is not interruptable
    /// (`ObjectData::interruptable`); one between two others goes on. One
    /// player alone talks on, as the originals do (R1: Dantooine's ruins
    /// computer answers its puzzle while the droids still shoot).
    pub(super) fn hurt(&mut self, body: ObjectId) {
        let view = self.view;
        if !view.together() {
            return;
        }
        self.interrupt(body, "its listener was hurt", |c| {
            (view.seat_of(c.listener).is_some()
                && view.facts(c.owner).is_none_or(|facts| facts.interruptable))
            .then_some(Interrupted::Cut)
        });
    }

    /// `body`'s seat let it go (R12): the player's own conversation it
    /// listens in is cut short when it died; when its player dropped or it
    /// left the party, cut short, or, locked, handed to the host so the
    /// scene still finishes (`hand_over`). Its trade ends either way: a
    /// store a guest left would open again for whoever drives the body
    /// next, and the dead do not shop.
    pub(super) fn released(&mut self, body: ObjectId, why: Release) {
        self.state.end_trade(body.0);
        let said = match why {
            Release::Dropped => "its player dropped",
            Release::Left => "its listener left the party",
            Release::Died => "its listener died",
        };
        self.interrupt(body, said, |c| {
            Some(if c.locked && why != Release::Died {
                Interrupted::Handed
            } else {
                Interrupted::Cut
            })
        });
    }

    /// Something happened to `body` (`hurt`, `released`): each player's own
    /// conversation it listens in goes as `rule` has it (none: on), cut
    /// short noted with `why`; one it joined it steps out of, its request
    /// going with it. Party conversations go on.
    pub(super) fn interrupt(
        &mut self,
        body: ObjectId,
        why: &'static str,
        rule: impl Fn(&Conversation) -> Option<Interrupted>,
    ) {
        let (mut cut, mut handed) = (Vec::new(), Vec::new());
        let seat = self.view.seat_of(body);
        for c in self
            .state
            .talk
            .conversations
            .iter_mut()
            .filter(|c| !c.party)
        {
            if c.listener != body {
                c.step_out(body, seat);
                continue;
            }
            match rule(c) {
                Some(Interrupted::Cut) => cut.push(c.id),
                Some(Interrupted::Handed) => handed.push(c.id),
                None => {}
            }
        }
        self.cut(cut, why);
        for id in handed {
            self.hand_over(id);
        }
    }

    /// A locked conversation whose player is gone goes to the host
    /// (`hand_to_host`, R12). While the host stands in another locked one
    /// it waits on the host instead (R12): held (`waits_for` the host's
    /// character, its listener no seat's body any more), it goes over as
    /// soon as that one ends (`advance`), as a request waits. Its request
    /// goes either way: the host takes it over.
    pub(super) fn hand_over(&mut self, id: u32) {
        if self.host_locked_elsewhere(id) {
            let host = self.host()[0];
            if let Some(at) = self.state.talk.index(id) {
                let c = &mut self.state.talk.conversations[at];
                c.waits_for = host;
                c.asking = ObjectId::INVALID;
            }
        } else {
            self.hand_to_host(id, "the host took over another");
        }
    }

    /// Whether the host stands in a locked conversation other than `id`.
    pub(super) fn host_locked_elsewhere(&self, id: u32) -> bool {
        let host = self.host();
        self.state
            .talk
            .conversations
            .iter()
            .any(|c| c.id != id && c.locked && host.iter().any(|body| c.holds(*body)))
    }

    /// Conversation `id` on hold (`waits_for`) asks again: one waiting to
    /// go over to the host (its listener no seat's body, `hand_over`) goes
    /// once the host is free, or is its player's again should it come back
    /// first; a line waiting on a party member (R4, `waits_for` its
    /// speaker) plays once it may, its replies offered as they stand then.
    pub(super) fn waited(&mut self, id: u32) {
        let view = self.view;
        let Some(at) = self.state.talk.index(id) else {
            return;
        };
        let c = &self.state.talk.conversations[at];
        if !c.party && view.seat_of(c.listener).is_none() {
            if !self.host_locked_elsewhere(id) {
                self.state.talk.conversations[at].waits_for = ObjectId::INVALID;
                self.hand_to_host(id, "the host took over another");
            }
            return;
        }
        if c.waits_for != c.speaker {
            self.state.talk.conversations[at].waits_for = ObjectId::INVALID;
            return;
        }
        let (entry, speaker, owner, pc) = (c.entry, c.speaker, c.owner, c.pc);
        let library = self.library;
        let Some(dlg) = library.dialogue(&c.dialog) else {
            return;
        };
        if self.interjection(id, speaker) {
            let replies = self.offer(dlg, id, entry, owner, pc);
            self.show(id, entry, replies);
        }
    }

    /// A script asked for another module this step (R11): the guests' own
    /// conversations (no host body stands in them), and a dropped guest's
    /// waiting to go over to the host (`hand_over`), are cut short while
    /// their scripts' module is still here. The host's and the party's end
    /// with the module, without their scripts (`enter_module`), as one
    /// player's always have.
    pub(super) fn leaving_module(&mut self) {
        if self.out.new_module.is_none() {
            return;
        }
        let (view, host) = (self.view, self.host());
        let guests: Vec<u32> = self
            .state
            .talk
            .conversations
            .iter()
            .filter(|c| !c.party && (view.seat_of(c.listener).is_some() || c.waits_for == host[0]))
            .filter(|c| !host.iter().any(|body| c.holds(*body)))
            .map(|c| c.id)
            .collect();
        self.cut(guests, "the module changed");
    }
}
