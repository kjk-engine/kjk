//! The players' own actions for the scripts: trades, equipping and items,
//! answers, and talking to what they use.

use super::*;

/// The players' trading, equipping and item actions (`script::stores`,
/// `script::items`; whose a member's equipment and items are,
/// `items::may_dress`) in the order they came: whether any came to
/// something, and the items' casts; a refusal is told to its player.
pub(super) fn seat_actions(
    state: &mut State,
    library: &script::Library,
    view: &View,
    inputs: &crate::input::TickInput,
    party: &[(crate::input::Seat, ObjectId)],
    notices: &mut Vec<(Option<ObjectId>, crate::notice::Notice)>,
    out: &mut Out,
) -> (bool, Vec<script::Cast>) {
    let catalog = library.items();
    let mut handled = false;
    let mut casts = Vec::new();
    for (seat, action) in inputs.actions() {
        // Containers are `world::containers`'.
        if action.container().is_some() {
            continue;
        }
        let Some(&(_, player)) = party.iter().find(|(of, _)| of == seat) else {
            continue;
        };
        let done = match action {
            Action::Equip { .. } | Action::Unequip(_) => {
                let wearer = script::items::wearer(view, state, out, player);
                state
                    .wear(catalog, out, (player.0, &wearer), action.clone())
                    .inspect(|()| equipped(library, view, state, player, action))
            }
            // Another party member's equipment (I30 P14).
            Action::EquipOn { who, .. } | Action::UnequipFrom { who, .. } => {
                let (who, wearer) = (
                    ObjectId(*who),
                    script::items::wearer(view, state, out, ObjectId(*who)),
                );
                script::items::may_dress(view, state, *seat, player, who)
                    .and_then(|()| state.wear(catalog, out, (who.0, &wearer), action.clone()))
                    .inspect(|()| equipped(library, view, state, who, action))
            }
            // Security is `world::locks`'; a Use is `world::uses`'.
            Action::Unlock { .. } | Action::Use { .. } => continue,
            // Checked as the step's scripts run (`script::levelling`).
            Action::LevelUp(choices) => {
                state.levelling.push((player.0, choices.clone()));
                Ok(())
            }
            Action::LevelUpOn { who, choices } => {
                script::items::may_dress(view, state, *seat, player, ObjectId(*who))
                    .map(|()| state.levelling.push((*who, choices.clone())))
            }
            // The table told to the seat: as a script's open screen shows
            // it, else to look at.
            Action::ShowParty => {
                let roster = match &state.party.selection {
                    Some(open) => state.party.roster(&open.forced, true, &state.items),
                    None => state.party.roster(&[], false, &state.items),
                };
                let screen = crate::notice::Screen::PartySelection(roster);
                notices.push((Some(player), crate::notice::Notice::Show(screen)));
                continue;
            }
            // The scripts' own step takes these (`script::run_due`).
            Action::ChooseParty(_)
            | Action::SetLeader(_)
            | Action::Lead(_)
            | Action::Solo(_)
            | Action::Travel(_)
            | Action::Pazaak(_)
            | Action::Request { .. }
            | Action::Watch { .. } => continue,
            // The party's calls are `world::party`'s.
            Action::Break | Action::Resume | Action::Ready | Action::Force => continue,
            // A revive is `world::falls`'.
            // Traps' (`world::traps`).
            Action::Revive(_) | Action::Disarm(_) | Action::Recover(_) | Action::Mine(_) => {
                continue;
            }
            // The holster key is the moves' (`world::move_actors`), a race's
            // quit the minigame's (`world::minigame`).
            // The arena's orders are `world::arena`'s.
            Action::Holster | Action::Forfeit | Action::Arena(_) => continue,
            // The seat's acting body's sets, or a member's it dresses.
            Action::SwitchWeapons => {
                swap(library, view, state, out, acting(view, seat.0, player));
                Ok(())
            }
            Action::SwitchWeaponsOn { who } => {
                let who = ObjectId(*who);
                script::items::may_dress(view, state, *seat, player, who)
                    .map(|()| swap(library, view, state, out, who))
            }
            // TSL's Form chosen from the HUD (`combat::forms`): the seat's
            // acting body's, at once, one it knows, told by the game's line
            // for it.
            Action::Form(row) => {
                let body = acting(view, seat.0, player);
                let known = view
                    .thing(body)
                    .and_then(|thing| thing.character)
                    .map(|c| out.sheet(body, c.worn))
                    .filter(|sheet| {
                        crate::combat::forms::ALL.contains(row) && sheet.powers.has(*row)
                    });
                match known {
                    Some(mut sheet) => {
                        sheet.form = *row;
                        out.changes.push(Change::Sheet { who: body, sheet });
                        let line = crate::combat::forms::initiated(*row);
                        notices.push((Some(player), crate::notice::Notice::Line(line)));
                        Ok(())
                    }
                    None => continue,
                }
            }
            // A power cast from KOTOR's power list: queued on the seat's
            // acting body as `ActionCastSpellAtObject` queues it, the cast's
            // step refusing one it does not know or cannot pay for.
            Action::Cast { row, target } => {
                let cast = script::actions::Action::Cast {
                    row: *row,
                    target: script::actions::Target::Object(ObjectId(*target)),
                    fake: false,
                    cheat: false,
                };
                state.queue(acting(view, seat.0, player), cast);
                Ok(())
            }
            // KOTOR runs its screen script each time an in-game screen opens.
            Action::ScreenOpened => {
                let script = script::presentation::SCREEN_OPENED;
                state.pending.push(Pending::new(script, script::MODULE));
                continue;
            }
            Action::Fit {
                item,
                upgrade,
                replace,
            } => script::items::may_upgrade(view, state, *seat, player, *item)
                .and_then(|()| state.fit(catalog, out, *item, *upgrade, *replace)),
            Action::Unfit { item, row } => {
                script::items::may_upgrade(view, state, *seat, player, *item)
                    .and_then(|()| state.unfit(catalog, out, *item, *row))
            }
            // An upgrade bench's visit, made whole; its wearer's as it was.
            Action::Assemble { item, changes } => {
                script::items::may_upgrade(view, state, *seat, player, *item).and_then(|()| {
                    let wearer = state
                        .items
                        .get(item)
                        .filter(|held| held.slot.is_some())
                        .map(|held| script::items::wearer(view, state, out, ObjectId(held.holder)));
                    state.assemble(catalog, out, (*item, changes), wearer.as_ref())
                })
            }
            // TSL's workbench: done, the bench told again as it stands, with
            // its sound.
            Action::Breakdown { bench, item } => state
                .breakdown(library, view, bench, *item)
                .map(|()| notices.extend(told(state, bench, player, script::workbench::BROKEN))),
            Action::Create { bench, row } => state
                .create(library, view, bench, *row)
                .map(|()| notices.extend(told(state, bench, player, script::workbench::MADE))),
            Action::UseItem { who, item, target } => {
                let who = ObjectId(*who);
                script::items::may_dress(view, state, *seat, player, who)
                    .and_then(|()| {
                        state.use_item(library, view, out, who, *item, ObjectId(*target))
                    })
                    .map(|cast| casts.extend(cast))
            }
            _ => state
                .trade(library, player.0, action.clone())
                .map(|got| notices.extend(got.told().map(|notice| (None, notice)))),
        };
        match done {
            Ok(()) => handled = true,
            Err(notice) => notices.push((Some(player), notice)),
        }
    }
    (handled, casts)
}

/// The body seat `seat` acts through: the companion it leads, else
/// `player`, its own character.
pub(super) fn acting(view: &View, seat: u8, player: ObjectId) -> ObjectId {
    view.seats
        .iter()
        .find(|(of, _)| *of == seat)
        .map_or(player, |(_, body)| *body)
}

/// A player's swap of `who`'s weapon sets (TSL's `AddSwitchWeaponsAction`):
/// none while it takes part in a conversation (nwscript.nss: "does not
/// work in dialogs"), quietly; else the sets exchanged now, and in a fight
/// the round's second it takes (`Change::Swapped`).
pub(super) fn swap(
    library: &script::Library,
    view: &View,
    state: &mut State,
    out: &mut Out,
    who: ObjectId,
) {
    if state.talk.with(who, view.host(&state.party)).is_some() {
        return;
    }
    script::switch_weapons(library, view, state, out, who);
    out.changes.push(Change::Swapped { who });
}

/// The workbench `bench` as it stands and the bench's sound `sound`, told
/// to its player after a breakdown or an item made (its screen keeps what
/// it opened with).
pub(super) fn told(
    state: &State,
    bench: &crate::action::Workbench,
    player: ObjectId,
    sound: &str,
) -> [(Option<ObjectId>, crate::notice::Notice); 2] {
    let shown = crate::notice::Notice::Show(state.bench(bench, None, false));
    [
        (Some(player), shown),
        (Some(player), script::workbench::sound(sound)),
    ]
}

/// Each seat's new answer byte: its command's when the count in it moved
/// since the last one heard from that seat (the first heard is where
/// counting starts), with the body the seat drives (`INVALID` for a
/// watching seat, I30 P3). `dialogue::advance` lets a conversation's
/// player answer it and any other seat mark.
pub(super) fn answers(
    state: &mut State,
    inputs: &crate::input::TickInput,
    bodies: &[(crate::input::Seat, ObjectId)],
) -> BTreeMap<u8, Answer> {
    // A seat with no command has dropped: its marks go, and its next byte
    // starts the count again.
    let dropped: Vec<u32> = state
        .talk
        .heard
        .keys()
        .filter(|seat| {
            u8::try_from(**seat).is_ok_and(|seat| inputs.get(crate::input::Seat(seat)).is_none())
        })
        .copied()
        .collect();
    for seat in dropped {
        state.talk.heard.remove(&seat);
        for c in &mut state.talk.conversations {
            c.marks.retain(|marker, _| u32::from(*marker) != seat);
        }
    }
    let mut answers = BTreeMap::new();
    for (seat, command) in inputs.iter() {
        let last = state.talk.heard.insert(u32::from(seat.0), command.reply);
        if last.is_some_and(|last| last != command.reply)
            && let Some(pick) = script::dialogue::pick_of(command.reply)
        {
            let by = bodies
                .iter()
                .find(|(of, _)| *of == seat)
                .map_or(ObjectId::INVALID, |(_, body)| *body);
            answers.insert(seat.0, Answer { by, pick });
        }
    }
    answers
}

/// A player used `object`.
/// - A creature with a dialogue hook runs it as the Aurora engine's
///   click: pattern −1 ("no string matched", nwscript.nss's
///   `GetListenPatternNumber`), the PC its last speaker. The games'
///   handlers answer with `BeginConversation` (`k_def_dialogue01` →
///   `k_ai_master`). reone starts the creature's conversation itself.
/// - A placeable with an `OnUsed` runs it, which starts what conversation
///   it will (Taris's sewer console: Mission's when she is with you, else
///   its own), the user its last user. reone starts a placeable's
///   conversation before its `OnUsed` [verify in the game].
/// - One with neither starts its conversation.
///
/// A door is talked to only when it stays locked (`world::interact`). The
/// conversation's PC is the user's own character, or the host's when a
/// guest's companion used it (I30 P8, `View::pc_for`). The user is
/// remembered as who asked.
pub(super) fn talk_to(view: &View, state: &mut State, object: ObjectId, user: ObjectId) {
    // A use of a body at a pazaak table asks to watch its match
    // (`View::watches`, R9a), as a use of a listener joins its conversation.
    if state.plays_pazaak(object) {
        return;
    }
    let player = view.pc_for(&state.party, user);
    let kind = view.thing(object).and_then(|thing| thing.kind);
    let hook = view.hook(object, Hook::Dialogue);
    let on_used = view.hook(object, Hook::Used);
    if let Some(script) = hook {
        state
            .last
            .insert((object.0, script::last::SPEAKER), player.0);
        // The run carries who asked, each use its own: the conversation
        // the hook begins takes it (`BeginConversation`, `Event::asker`).
        let mut pending = Pending::new(&script, object);
        pending.entering = Some(player);
        pending.number = -1;
        pending.leads = guest_body(view, user);
        pending.asker = Some(user);
        state.pending.push(pending);
    } else if let Some(script) = on_used {
        state.last.insert((object.0, script::last::USED), user.0);
        // Its run carries who asked too: a conversation it begins or
        // queues is the user's own (R1; Taris's sewer console queues one).
        // A guest's companion that used it leads the party in it, as in
        // `raise_by`.
        let mut pending = Pending::new(&script, object);
        pending.entering = Some(user);
        pending.leads = guest_body(view, user);
        pending.asker = Some(user);
        state.pending.push(pending);
    } else if let Some(dialog) = view.conversation(object) {
        state.talk.asked.push(script::dialogue::Start {
            owner: object,
            pc: player,
            asker: user,
            dialog: dialog.to_string(),
            bark: false,
        });
    } else if kind == Some(ObjectKind::Placeable) {
        state.last.insert((object.0, script::last::USED), user.0);
        raise_by(view, state, object, Hook::Used, Some(user), user);
    }
}
