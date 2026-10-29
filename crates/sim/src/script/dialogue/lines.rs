//! A line shown: its replies offered, its speaker and listener, their
//! turns, gestures and fades, the story's progress.

use super::*;

impl Step<'_> {
    /// The replies an entry offers now, in order.
    pub(super) fn offer(
        &mut self,
        dlg: &Dlg,
        id: u32,
        entry: u32,
        owner: ObjectId,
        pc: ObjectId,
    ) -> Vec<u32> {
        let Some(node) = dlg.entries.get(entry as usize) else {
            return Vec::new();
        };
        node.links
            .iter()
            .filter(|link| (link.index as usize) < dlg.replies.len())
            .filter(|link| self.holds(link, id, owner, pc))
            .map(|link| link.index)
            .collect()
    }

    /// The first object tagged `tag` (`script::tagged`).
    pub(super) fn first_tagged(&self, tag: &str) -> Option<ObjectId> {
        crate::script::tagged(self.view, &self.out.made, tag)
            .next()
            .map(|tagged| tagged.id)
    }

    /// Who says a node: `speaker_for`, else the owner (a line held for an
    /// interjection whose speaker left meanwhile; a link is never taken to
    /// a line whose speaker is not here, `speaker_here`).
    pub(super) fn speaker(&self, tag: &str, owner: ObjectId) -> ObjectId {
        self.speaker_for(tag, owner).unwrap_or(owner)
    }

    /// Who a node's `Speaker` names, as both executables find it: none
    /// named, the owner; the owner's own tag, the owner; else the object
    /// tagged so nearest the owner within `dialogue::speakers::REACH_M`,
    /// taken in the area's order as their search keeps its best (Q-0323:
    /// the first match's plain distance becomes the bar the later ones'
    /// squared distances must meet). One `SetForceAlwaysUpdate` flagged is
    /// taken however far, its distance then the bar (TSL's
    /// `GetNearestObjectByName` lets an object with that flag past the
    /// radius: Dhagon's call to the throne room). None when no one answers
    /// (a companion not in the party). `OWNER` and `PLAYER` are no keywords here. An
    /// owner without a place (none in the area) takes the first tagged
    /// [verify: the executables find none from an owner out of an area].
    pub(super) fn speaker_for(&self, tag: &str, owner: ObjectId) -> Option<ObjectId> {
        if tag.is_empty() {
            return Some(owner);
        }
        let Some(home) = self.view.thing(owner) else {
            return self.first_tagged(tag);
        };
        if home.tag() == key(tag) {
            return Some(owner);
        }
        let origin = home.position;
        let reach = rules::dialogue::speakers::REACH_M;
        let mut bar = reach * reach;
        let mut found = None;
        for tagged in crate::script::tagged(self.view, &self.out.made, tag) {
            let squared: f32 = (0..3)
                .map(|axis| (tagged.at[axis] - origin[axis]).powi(2))
                .sum();
            if squared <= bar || self.state.always_update.contains(&tagged.id.0) {
                bar = squared.sqrt();
                found = Some(tagged.id);
            }
        }
        found
    }

    /// Whether a link to `entry` may be taken for its speaker: the one its
    /// `Speaker` names is here (`speaker_for`). Both executables test it
    /// before the link's condition, which a line passed over never runs.
    pub(super) fn speaker_here(&self, dlg: &Dlg, entry: u32, owner: ObjectId) -> bool {
        dlg.entries
            .get(entry as usize)
            .is_some_and(|node| self.speaker_for(&node.speaker, owner).is_some())
    }

    /// Shows `entry`, times the line and runs its scripts; its replies are
    /// chosen once the line has played (`offer_replies`).
    pub(super) fn show(&mut self, id: u32, entry: u32) {
        let now = self.view.now_ms;
        let library = self.library;
        let Some(c) = self.state.talk.conversations.iter().find(|c| c.id == id) else {
            return;
        };
        let (owner, pc, dialog) = (c.owner, c.pc, c.dialog.clone());
        let player = self.player(c);
        let Some(dlg) = library.dialogue(&dialog) else {
            return;
        };
        let Some(node) = dlg.entries.get(entry as usize) else {
            return;
        };
        let speaker = self.speaker(&node.speaker, owner);
        if !self.interjection(id, speaker) {
            // Held, the line it waits to say kept.
            if let Some(at) = self.state.talk.index(id) {
                let c = &mut self.state.talk.conversations[at];
                (c.entry, c.speaker) = (entry, speaker);
                c.replies.clear();
                c.marks.clear();
            }
            return;
        }
        // The end script of an interjector's conversation cut short may
        // have ended this one.
        let Some(at) = self.state.talk.index(id) else {
            return;
        };
        let alien = library.alien_voice(dlg, node).is_some();
        let variant = self.state.talk.conversations[at].entry_variant(entry, alien);
        let voice = library.entry_voice(dlg, node, variant);
        let voiced = alien || !node.sound.is_empty() || !node.vo.is_empty();
        // A line that waits on its camera model's clip lasts its own timer
        // (`line_ms`) raised to the clip, as the K1 and TSL executables end a
        // line once its timer and every wait its flags name are done.
        let clip = (node.wait_flags & WAIT_ANIMATION != 0)
            .then(|| library.camera_clip(&dlg.camera_model, node.camera_animation))
            .flatten();
        let gestures = match dlg.animated_cut {
            true => Vec::new(),
            false => self.gestures(node, (owner, player)),
        };
        // One waiting on its gestures lasts the longest of their clips,
        // played once (none where the model has no length for its clip
        // [verify]); an animated cut's stunt clips are not measured.
        let gestured = (node.wait_flags & WAIT_ANIMS != 0)
            .then(|| {
                let played = |&(who, animation): &(ObjectId, u16)| {
                    library.clip_ms(self.view.appearance(who)?, animation)
                };
                gestures.iter().filter_map(played).max()
            })
            .flatten();
        let timed = line_ms(
            library,
            node,
            (voice.as_ref().map(|(_, ms)| *ms), voiced),
            dlg.animated_cut,
        );
        let length = timed
            .max(clip.map_or(0, u64::from))
            .max(gestured.map_or(0, u64::from));
        {
            let c = &mut self.state.talk.conversations[at];
            c.entry = entry;
            c.speaker = speaker;
            c.replies.clear();
            c.shown_ms = now;
            c.until_ms = now + length;
            c.auto = false;
            c.marks.clear();
            c.camera = 0;
            c.waits_for = ObjectId::INVALID;
            c.locked |= story_line(library, dlg, entry);
            c.angle = c.entry_angle(entry, node);
        }
        let listener = self.listener_for(node, speaker, &self.state.talk.conversations[at], player);
        self.says(at, speaker, listener);
        self.line(Line::Entry {
            ms: now,
            id,
            dialog: dialog.clone(),
            entry,
            speaker,
            voice: voice.map(|(name, _)| name.to_string()),
        });
        self.progress(node);
        self.node_fade(&node.fade);
        for (who, animation) in gestures {
            self.out.changes.push(crate::script::Change::Animate {
                who,
                play: (animation, u32::try_from(length).unwrap_or(u32::MAX)),
            });
        }
        for call in [&node.script, &node.script2] {
            self.call(call, id, owner, pc);
        }
    }

    /// The replies of the line conversation `id` shows, chosen now it has
    /// played: TSL's `CSWSDialog::SendDialogEntry` sets the line's delay,
    /// runs its Script and Script2, then `CSWSObject::SendDialogReplies`,
    /// which waits while `IsDialogDelay` holds before
    /// `CSWSDialog::SendDialogReplies` checks each reply's conditions; so
    /// what the line's scripts set, and what was done while it played, is
    /// what they read (Ralon's E37, `a_ralo_dialogue`, sets the local his
    /// [Awareness] reply checks) [verify in K1's exe]. One silent reply is
    /// taken at once (`Conversation::auto`); others are offered.
    pub(super) fn offer_replies(&mut self, id: u32) {
        let library = self.library;
        let Some(c) = self.state.talk.conversations.iter().find(|c| c.id == id) else {
            return;
        };
        let (entry, owner, pc, dialog) = (c.entry, c.owner, c.pc, c.dialog.clone());
        let Some(dlg) = library.dialogue(&dialog) else {
            return;
        };
        let replies = self.offer(dlg, id, entry, owner, pc);
        let auto = replies.len() == 1 && dlg.replies[replies[0] as usize].is_silent();
        let Some(at) = self.state.talk.index(id) else {
            return;
        };
        let c = &mut self.state.talk.conversations[at];
        c.replies = replies.clone();
        c.auto = auto;
        if !replies.is_empty() {
            c.offer_angles(dlg, &replies);
        }
        if !auto && !replies.is_empty() {
            self.line(Line::Offered {
                ms: self.view.now_ms,
                id,
                dialog,
                replies,
            });
        }
    }

    /// A line's `AnimList` (outside animated cutscenes, which stage their
    /// own): each participant (`owner`, `player` as `Step::player` has it,
    /// else by tag; one not found is left out, as reone does) and its
    /// `dialoganimations.2da` row's animation, which it plays for the line's
    /// length, as `PlayAnimation` would.
    pub(super) fn gestures(
        &self,
        node: &Node,
        (owner, player): (ObjectId, ObjectId),
    ) -> Vec<(ObjectId, u16)> {
        let library = self.library;
        node.anims
            .iter()
            .filter_map(|(tag, value)| {
                let animation = library.dialog_animation(*value)?;
                let who = participant(tag, (owner, player), |tag| self.first_tagged(tag))?;
                Some((who, animation))
            })
            .collect()
    }

    /// A node's own fade as it plays, before its scripts (theirs then
    /// win): 3 fades in, holding its colour through its delay (a first
    /// entry's opens a scene from black), 4 fades out, 1 covers at once, 2
    /// uncovers at once; marked as the conversation's, which lifts it as it
    /// ends (`finish`). Any other kind does nothing.
    pub(super) fn node_fade(&mut self, fade: &NodeFade) {
        let (length, out) = match fade.kind {
            NodeFade::COVER => (0.0, true),
            NodeFade::UNCOVER => (0.0, false),
            NodeFade::IN => (fade.length, false),
            NodeFade::OUT => (fade.length, true),
            _ => return,
        };
        let set = self
            .out
            .set_fade(self.view, (fade.delay, length), fade.colour, out);
        set.conversation = true;
        if !out {
            set.from = 1.0;
        }
    }

    /// What a node does to the story as it plays: its quest reaches its
    /// entry (never back: a lower entry than the quest's leaves it), with
    /// the entry's share of the quest's XP; its plot gives its share of
    /// the plot's XP.
    pub(super) fn progress(&mut self, node: &Node) {
        let entry = node.quest_entry as i32;
        if self.state.set_quest(&node.quest, entry, false) {
            self.line(Line::Journal {
                ms: self.view.now_ms,
                quest: key(&node.quest),
                entry,
            });
            self.out.statuses.push(crate::snapshot::StatusKind::Journal);
            let xp = self.library.quest_xp(&node.quest, entry);
            self.out.story_xp(self.state, xp);
            crate::script::xp::tell_pc_level(self.library, self.view, self.state);
        }
        if let Some(xp) = self.library.experience().plot(node.plot_index) {
            self.out
                .story_xp(self.state, (xp as f32 * node.plot_xp) as i32);
            crate::script::xp::tell_pc_level(self.library, self.view, self.state);
        }
    }

    /// Whom `speaker`'s line `node` is said to (S2): the one its `Listener`
    /// names (`participant`); else the previous line's speaker
    /// (`Conversation::pair`); else, at the first line or the speaker
    /// talking twice running, `pc` (`Step::player`). A line held for a
    /// speaker who left meanwhile is said by the owner (`speaker`); its
    /// `Listener` naming that owner then names no one either, so the line
    /// is not said to the one saying it. A line that names its own speaker
    /// as its listener keeps it (ten K1 entries do; the camera films that
    /// pair at one point, `dialog_same_point`).
    pub(super) fn listener_for(
        &self,
        node: &Node,
        speaker: ObjectId,
        c: &Conversation,
        pc: ObjectId,
    ) -> ObjectId {
        let stood_in = self.speaker_for(&node.speaker, c.owner).is_none();
        let named = (!node.listener.is_empty())
            .then(|| participant(&node.listener, (c.owner, pc), |tag| self.first_tagged(tag)))
            .flatten()
            .filter(|&named| !(stood_in && named == speaker));
        let previous = c.pair.0;
        named.unwrap_or(if previous == ObjectId::INVALID || previous == speaker {
            pc
        } else {
            previous
        })
    }

    /// `speaker` starts a line to `listener` in the conversation at `at`
    /// (S2): they are its pair, and they turn to each other (`face_pair`).
    pub(super) fn says(&mut self, at: usize, speaker: ObjectId, listener: ObjectId) {
        let c = &mut self.state.talk.conversations[at];
        (c.pair, c.listener_now) = ((speaker, listener), listener);
        self.face_pair(at, speaker, listener);
    }

    /// At a line's start (S2), outside an `AnimatedCut` DLG: the listener
    /// turns to the speaker, then the speaker to the listener (`turn`), at
    /// once.
    pub(super) fn face_pair(&mut self, at: usize, speaker: ObjectId, listener: ObjectId) {
        let c = &self.state.talk.conversations[at];
        let animated = self
            .library
            .dialogue(&c.dialog)
            .is_none_or(|dlg| dlg.animated_cut);
        if animated {
            return;
        }
        let turns: Vec<crate::script::Change> = [(listener, speaker), (speaker, listener)]
            .into_iter()
            .filter_map(|(who, to)| self.turn(who, to, c))
            .collect();
        self.out.changes.extend(turns);
    }

    /// How `who`, a body, turns to `to` at a line's start (S2), if it
    /// turns: not on the lock-orientation list (`lock_turn`), nor kept from
    /// turning (TSL's `SetOrientOnClick`, `State::unoriented`); with a head
    /// arc (its template's `Body::head_arc_h`, a player's by its persona's
    /// appearance, `Library::head_arc`) its head may follow, only by
    /// θ − (arc + 1°) where the angle θ to `to` is past the arc, not at all
    /// within it; fully on the head-follow lock list (`lock_head`), or with
    /// no arc ([verify] a 0 arc read as a head that can't follow).
    pub(super) fn turn(
        &self,
        who: ObjectId,
        to: ObjectId,
        c: &Conversation,
    ) -> Option<crate::script::Change> {
        let (from, toward) = (self.view.thing(who)?, self.view.thing(to)?);
        let body = matches!(from.kind, None | Some(ObjectKind::Creature));
        if who == to
            || !body
            || c.lock_turn.contains(&who)
            || self.state.unoriented.contains(&who.0)
        {
            return None;
        }
        let (a, b) = (from.position, toward.position);
        let aim = crate::math::atan2_deg(b[1] - a[1], b[0] - a[0]);
        let arc = match self.view.facts(who).and_then(|facts| facts.body) {
            Some(body) => body.head_arc_h,
            None => (from.persona.as_ref())
                .map_or(0.0, |persona| self.library.head_arc(persona.appearance)),
        }
        .max(0.0);
        let yaw = if arc == 0.0 || c.lock_head.contains(&who) {
            aim
        } else {
            let off = (aim - from.yaw + 180.0).rem_euclid(360.0) - 180.0;
            if off.abs() <= arc {
                return None;
            }
            from.yaw + off - off.signum() * (arc + rules::dialogue::cameras::HEAD_MARGIN)
        };
        Some(crate::script::Change::Face { who, yaw })
    }
}
