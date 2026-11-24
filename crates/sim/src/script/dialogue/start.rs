//! A conversation begun, or a body joining one under way.

use super::*;

impl Step<'_> {
    /// Begins `start` (`open`), its scripts run for its listener as their
    /// user (`Talk::user`) until it is among the conversations.
    pub(super) fn begin(&mut self, start: Start) {
        let before = self.state.talk.user.take();
        self.open(start);
        self.state.talk.user = before;
    }

    fn open(&mut self, start: Start) {
        let now = self.view.now_ms;
        let library = self.library;
        let Some(dlg) = library.dialogue(&start.dialog) else {
            self.line(Line::Refused {
                ms: now,
                dialog: start.dialog,
                why: "no such conversation",
            });
            return;
        };
        // A player's use starts their own conversation, its listener their
        // body; the game's, or a cutscene's, holds every seat, its listener
        // the PC (R1, R2). One with someone else as its PC (two others
        // talking) is neither.
        let (mains, lent) = (self.view.main_characters(&self.state.party), self.lent());
        let private = self.view.seat_of(start.asker).is_some() && !cutscene(dlg);
        let party = !private && party_pc(&mains, lent, start.pc);
        let listener = if private { start.asker } else { start.pc };
        self.state.talk.user = Some(listener);
        if !start.bark && private && self.join(&start) {
            return;
        }
        // The host's two bodies are one: neither starts while the other talks.
        let (talk, host) = (&self.state.talk, self.host());
        let busy = |id: ObjectId| id != ObjectId::INVALID && talk.with(id, host).is_some();
        let listeners = if host.contains(&listener) {
            host
        } else {
            [listener; 2]
        };
        if !start.bark && (busy(start.owner) || listeners.into_iter().any(busy)) {
            self.line(Line::Refused {
                ms: now,
                dialog: start.dialog,
                why: "already talking",
            });
            return;
        }
        // A conversation beginning takes both its parties out of stealth
        // (both games' executables), unless TSL's `SetKeepStealthInDialog`
        // holds.
        if !start.bark && !self.state.keep_stealth {
            for who in [start.owner, listener] {
                self.out
                    .changes
                    .push(crate::script::Change::Unstealth { who });
            }
        }
        let id = self.state.talk.next;
        self.state.talk.next += 1;
        if let Some(cue) = start.cue {
            self.line(Line::Cued { ms: now, id, cue });
        }
        let first = dlg
            .starts
            .iter()
            .filter(|link| (link.index as usize) < dlg.entries.len())
            .find(|link| {
                self.speaker_here(dlg, link.index, start.owner)
                    && self.holds(link, id, start.owner, start.pc)
            })
            .map(|link| link.index);
        // The module's fade in held for a conversation goes on whether this
        // one shows a line, speaks one over the scene or has nothing to say:
        // the games let go of the hold as a conversation is started, not
        // only once its first line shows. The Ebon Hawk's vision talk after
        // the last Star Map holds it for a conversation with no line left
        // to say (issue 645).
        self.out.release_held_fade(self.view);
        let Some(entry) = first else {
            // Nothing to say: it ends where it starts, as a normal end the
            // whole area hears [verify: that K1's exe runs the end scripts
            // of a conversation with no entry to start from].
            if !start.bark {
                let heard = (false, Heard::Area);
                self.finish(id, (start.owner, start.pc), &start.dialog, heard);
            }
            return;
        };
        let replies = self.offer(dlg, id, entry, start.owner, start.pc);
        let node = &dlg.entries[entry as usize];
        let speaker = self.speaker(&node.speaker, start.owner);
        let one_liner = replies.len() == 1 && {
            let reply = &dlg.replies[replies[0] as usize];
            says_nothing(library, reply) && reply.links.is_empty()
        };
        if start.bark || one_liner {
            self.progress(node);
            let text = Said::Entry {
                dialog: start.dialog.clone(),
                entry,
            };
            self.line(Line::Bark {
                ms: now,
                who: speaker,
                text: text.clone(),
                conversation: !start.bark,
            });
            self.state.talk.barks.push(Bark {
                who: speaker,
                until_ms: now + bark_ms(library, &text),
                text,
            });
            // A conversation of one line runs its scripts as a longer one
            // would (the Jedi droid's opens the enclave's door), its silent
            // reply's too, taken as it ends; a bark (`SpeakOneLiner…`) only
            // speaks.
            if one_liner && !start.bark {
                let reply = &dlg.replies[replies[0] as usize];
                for call in [&node.script, &node.script2, &reply.script, &reply.script2] {
                    self.call(call, id, start.owner, start.pc);
                }
                let heard = (false, Heard::Owner);
                self.finish(id, (start.owner, start.pc), &start.dialog, heard);
            }
            return;
        }
        if party {
            self.cut_short();
        }
        // Those far off come to the PC, or to the body whose use started
        // it, or to the seat's body whose coming into a trigger did (its
        // `Cue`): a player's own brings its seat's companions, the first
        // party one everyone (I30 P7, P8). A guest walking up to Tatooine's
        // Tanis Venn sets his trigger's call to the host's character going
        // (`k_ptat_taniscall`); brought to the host, out of the trigger,
        // it set the call off again each time it came back. Only with more
        // than one seat playing (S9): one player's party stays where it
        // stands, as in KOTOR.
        let gathers = if private {
            self.view
                .seat_of(listener)
                .map(crate::script::Gathered::Companions)
        } else if party && self.state.talk.party().is_none() {
            Some(crate::script::Gathered::Everyone)
        } else {
            None
        }
        .filter(|_| self.view.together());
        if matches!(gathers, Some(crate::script::Gathered::Everyone)) {
            self.close_stores();
        }
        let entering = start
            .cue
            .map(|cue| cue.entering)
            .filter(|&body| self.view.seat_of(body).is_some());
        let around = if start.asker != ObjectId::INVALID {
            start.asker
        } else {
            entering.unwrap_or(start.pc)
        };
        self.state.talk.conversations.push(Conversation {
            id,
            dialog: start.dialog,
            owner: start.owner,
            pc: start.pc,
            asker: start.asker,
            entry,
            speaker,
            shown_ms: now,
            until_ms: now,
            listener,
            party,
            locked: party || story_abort(library, dlg),
            ..Conversation::default()
        });
        if let Some(who) = gathers {
            self.out
                .changes
                .push(crate::script::Change::Gather { around, who });
        }
        // One player at a time uses a computer, so nobody joins its screen.
        if private
            && !is_computer(dlg)
            && self.view.seat_of(listener) == Some(crate::world::LEADER_SEAT.0)
        {
            self.bring_near(id);
        }
        self.out.lift_fade(self.view);
        self.show(id, entry);
    }

    /// The first seat began conversation `id` of its own (issue 636): every
    /// other seat's body that is free (`free_to_join`), stands within
    /// `GATHER_BEYOND_M` of its listener and sees it joins it at once, in
    /// seat order, as a player who walked up would. Only while more than
    /// one seat plays a body (`together`).
    fn bring_near(&mut self, id: u32) {
        let view = self.view;
        let Some(at) = self.state.talk.index(id).filter(|_| view.together()) else {
            return;
        };
        let listener = self.state.talk.conversations[at].listener;
        let Some(centre) = view.thing(listener).map(|thing| thing.position) else {
            return;
        };
        let near = |body: ObjectId| {
            view.thing(body).is_some_and(|thing| {
                let apart = crate::math::length(crate::math::sub(thing.position, centre));
                apart <= crate::world::coop_talk::GATHER_BEYOND_M
                    && (view.sees)(thing.position, centre)
            })
        };
        let coming: Vec<ObjectId> = (view.seats.iter())
            .filter(|(seat, body)| {
                *seat != crate::world::LEADER_SEAT.0
                    && *body != listener
                    && self.free_to_join(*seat, *body)
                    && near(*body)
            })
            .map(|(_, body)| *body)
            .collect();
        let (mains, lent) = (view.main_characters(&self.state.party), self.lent());
        let c = &mut self.state.talk.conversations[at];
        for body in coming {
            c.take_in(body, party_pc(&mains, lent, body));
        }
    }

    /// Whether seat `seat`, its body `body`, is free to join a conversation
    /// by standing near it (issue 636). It is not when its player turned
    /// "Join nearby conversations" off or has a screen open
    /// (`Talk::quiet`, `Talk::screens`), or when its body is down, fights
    /// (its own fight, or a hostile one near it), takes part in a
    /// conversation, trades at a store, plays or watches a pazaak match, or
    /// holds a container open (`View::looting`).
    fn free_to_join(&self, seat: u8, body: ObjectId) -> bool {
        let (view, state) = (self.view, &*self.state);
        let character = view.thing(body).and_then(|thing| thing.character);
        let busy = state.talk.quiet.contains(&seat)
            || state.talk.screens.contains(&seat)
            || character.is_none_or(|character| character.downed || character.in_combat)
            || view.embattled.contains(&body)
            || state.talk.with(body, self.host()).is_some()
            || state.trades.contains_key(&body.0)
            || state.plays_pazaak(body)
            || state.watching(body).is_some()
            || view.looting & crate::world::party::bit(crate::input::Seat(seat)) != 0;
        !busy
    }

    /// A player's use of one in another player's own conversation (R3):
    /// its body joins that conversation instead of starting one. Whether
    /// it did; not when it already talks, or that one holds every seat. A
    /// PC joining answers its story replies itself: no one asks the host in
    /// any more (R5).
    pub(super) fn join(&mut self, start: &Start) -> bool {
        let view = self.view;
        let (pc, host) = (
            party_pc(
                &view.main_characters(&self.state.party),
                self.lent(),
                start.asker,
            ),
            self.host(),
        );
        let talk = &mut self.state.talk;
        if talk.with(start.asker, host).is_some() {
            return false;
        }
        let theirs = talk
            .conversations
            .iter_mut()
            .find(|c| c.involves(start.owner) && !c.party && view.seat_of(c.listener).is_some());
        let Some(c) = theirs else {
            return false;
        };
        c.take_in(start.asker, pc);
        true
    }
}
