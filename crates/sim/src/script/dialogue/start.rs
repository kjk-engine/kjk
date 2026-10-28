//! A conversation begun, or a body joining one under way.

use super::*;

impl Step<'_> {
    pub(super) fn begin(&mut self, start: Start) {
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
        let (pcs, lent) = (self.view.pcs(&self.state.party), self.lent());
        let private = self.view.seat_of(start.asker).is_some() && !cutscene(dlg);
        let party = !private && party_pc(&pcs, lent, start.pc);
        let listener = if private { start.asker } else { start.pc };
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
        let first = dlg
            .starts
            .iter()
            .filter(|link| (link.index as usize) < dlg.entries.len())
            .find(|link| {
                self.speaker_here(dlg, link.index, start.owner)
                    && self.holds(link, id, start.owner, start.pc)
            })
            .map(|link| link.index);
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
            reply.is_silent() && reply.links.is_empty()
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
        // it: a player's own brings its seat's companions, the first party
        // one everyone (I30 P7, P8). Only with more than one seat playing
        // (S9): one player's party stays where it stands, as in KOTOR.
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
        let around = if start.asker == ObjectId::INVALID {
            start.pc
        } else {
            start.asker
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
        self.out.lift_fade(self.view);
        self.show(id, entry, replies);
    }

    /// A player's use of one in another player's own conversation (R3):
    /// its body joins that conversation instead of starting one. Whether
    /// it did; not when it already talks, or that one holds every seat. A
    /// PC joining answers its story replies itself: no one asks the host in
    /// any more (R5).
    pub(super) fn join(&mut self, start: &Start) -> bool {
        let view = self.view;
        let (pc, host) = (
            party_pc(&view.pcs(&self.state.party), self.lent(), start.asker),
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
