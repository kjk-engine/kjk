//! What players do to the module's objects: uses, touches, the key's
//! use, doorways.

use super::*;

/// What players do to the module's objects, in seat order (JA's
/// How far below a creature a floor is looked for before it is taken to
/// stand over nothing (metres).
pub(super) const FLOOR_SEARCH_M: f32 = 100.0;

/// `ClientThink` after the move: `TryUse`, then touches, then
/// `G_TouchTriggers`):
/// - holding use tries every 100 ms: a point traced 64 units along the
///   view from the eye; a door it meets opens if it can, away from its
///   user (`ObjectData::opening`), or, if it leads out, asks for the
///   transition (clicking such a door in KOTOR); a locked one fails to
///   open (`locks`: its key), as a locked container does (what using
///   each kind does is `uses::use_object`);
/// - a seat's `Action::Use` is checked (the player up; `uses::applies`:
///   a verb the object takes; `uses::may_use`: within reach, in sight) and then means what the use key means; a door
///   is not closed on anyone standing in it;
/// - running into a door that leads out and is not locked asks for the
///   transition, every step it runs into it;
/// - standing in an open door's doorway (`in_doorway`) or in a trigger
///   (`ObjectData::contains`: over its outline, on its floor) makes the
///   seat a tenant; entering one that leads out asks for the transition,
///   once.
///   Leaving it ends the tenancy.
///
/// "Leads out" is any link to a module, its own included (reone installs
/// the module again, arriving at the tag). Every walk into an exit goes to
/// the ready check (`party::ready`), which proposes the move, waits for
/// the party, and asks for it (`Snapshot::transition`) when the party goes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn interact(
    clock: Res<Clock>,
    inputs: Res<Inputs>,
    level: Res<Static>,
    touched: Res<Touched>,
    bumped: Res<Bumped>,
    mut module: ResMut<Module>,
    (mut opened, mut used, mut asked): (
        ResMut<scripts::Opened>,
        ResMut<scripts::Used>,
        ResMut<party::Asked>,
    ),
    (mut closed, mut clicked, mut blocked): (
        ResMut<scripts::Closed>,
        ResMut<scripts::Clicked>,
        ResMut<scripts::Blocked>,
    ),
    reviving: Res<falls::Reviving>,
    mut players: Query<(&Id, &Avatar, &mut Player, Option<&Fighting>)>,
    mut objects: Query<(&Id, &mut Object)>,
    creatures: Query<(&Id, &Player, &Fighting), Without<Avatar>>,
    (scripts, tabled): (Res<scripts::Scripts>, Res<Tabled>),
    (talking, holding, embattled, held): (
        Res<Talking>,
        Res<Holding>,
        Res<party::Embattled>,
        Res<scene::Held>,
    ),
) {
    opened.0.clear();
    used.0.clear();
    closed.0.clear();
    clicked.0.clear();
    blocked.0.clear();
    if level.0.module.is_none() {
        return;
    }
    let mut objects: Vec<_> = objects.iter_mut().collect();
    objects.sort_by_key(|(_, object)| object.index);
    let find = |objects: &[(&Id, Mut<Object>)], index: u32| {
        objects
            .binary_search_by_key(&index, |(_, object)| object.index)
            .ok()
    };
    let mut players: Vec<_> = players.iter_mut().collect();
    players.sort_by_key(|(_, avatar, _, _)| avatar.0);
    // While a conversation holds the world the party uses and opens
    // nothing (S4): every seat's body is the party's.
    let held = held.world;
    // After a script's `NoClicksFor`, a click uses no placeable and talks to
    // no one until its game time is up (KOTOR checks it in those two alone:
    // doors still open). Every seat is the local player here.
    let no_clicks = clock.time_ms < scripts.state.no_clicks_until;
    let unclickable = |kind: ObjectKind, verb: Verb| {
        no_clicks && (kind == ObjectKind::Placeable || verb == Verb::Talk)
    };
    let party_body = |by: SimId| {
        players.iter().any(|(id, ..)| id.0 == by)
            || creatures
                .iter()
                .any(|(id, _, fighting)| id.0 == by && fighting.0.mind.leader.is_some())
    };
    // Where every character stands (units), for a door opened away from
    // one or closed on one, and a creature talked to.
    let standing: Vec<(SimId, [f32; 3], f32, bool)> = players
        .iter()
        .map(|(id, _, player, fighting)| {
            let alive = fighting.is_none_or(|fighting| fighting.0.alive());
            (id.0, player.0.origin, player.0.maxs_z, alive)
        })
        .chain(creatures.iter().map(|(id, player, fighting)| {
            (id.0, player.0.origin, player.0.maxs_z, fighting.0.alive())
        }))
        .collect();
    let body_at = |id: SimId| {
        standing
            .iter()
            .find(|(of, ..)| *of == id)
            .map(|(_, origin, ..)| scale(*origin, UNIT))
    };
    // A creature walking a route opens a door it runs into, if the door
    // opens (KOTOR's walk through the doors on their way; one leading out
    // too, its link a player's use's, `walk::passable`), as its opener, away
    // from it; a party member not while the world is held.
    for &(by, entity) in &bumped.0 {
        if held && party_body(by) {
            continue;
        }
        if let Some(at) = find(&objects, entity) {
            let door = objects[at].0.0;
            let object = &mut objects[at].1;
            let opening = object
                .facts
                .as_ref()
                .filter(|facts| object.state.opens(facts.opens))
                .map(|facts| facts.opening(body_at(by)));
            if object.kind != ObjectKind::Door || object.state.state != 0 {
                continue;
            }
            // Blocked by it, as KOTOR's walker is: its `OnBlocked` hears.
            blocked.0.push((by, door));
            if let Some(state) = opening {
                object.state.state = state;
                opened.0.push((door, Some(by), true));
            }
        }
    }
    // The living creatures a player may talk to: their bodies, met by the
    // use.
    // The companion leading uses for its player, from its own body (KOTOR's
    // controlled character).
    let lent = lent(&scripts.state.party).and_then(|id| {
        creatures
            .iter()
            .find(|(of, ..)| of.0 == id)
            .map(|(_, body, _)| (id, body.0))
    });
    let talkers = talkers(clock.time_ms, lent.map(|(id, _)| id), creatures.iter());
    // The seats' bodies, the companion leading among them (R36), for those
    // at a pazaak table (`with_tables`), and who they are to watching's
    // rule (`watchable`).
    let seated: Vec<(SimId, MotionState)> = if tabled.0.is_empty() {
        Vec::new()
    } else {
        let bodies = players.iter().map(|(id, _, player, _)| (id.0, player.0));
        bodies.chain(lent).collect()
    };
    let seats: Vec<(Seat, SimId, Option<&Fighter>)> = (players.iter())
        .map(|(id, avatar, _, fighting)| (avatar.0, id.0, fighting.map(|fighting| &fighting.0)))
        .collect();
    let lent_fighter = lent.and_then(|(id, _)| {
        (creatures.iter())
            .find(|(of, ..)| of.0 == id)
            .map(|(_, _, fighting)| (id, &fighting.0))
    });
    let at_tables = (&talking.0, &holding.0, &tabled.0[..], &embattled.0[..]);
    // An ask lasts one step: the host takes it, or it is asked again.
    module.pending = None;
    // The walks into exits, in seat order, each with the exit whose tenant
    // entering made the seat (a trigger, an open doorway): the ready check
    // takes them (`party::ready`).
    let mut asks: Vec<party::Ask> = Vec::new();
    let ask = |via: &Id, link: &crate::module::Link, seat: Seat, entered: Option<u32>| party::Ask {
        transition: Transition {
            seat,
            via: via.0,
            module: link.module.clone(),
            tag: link.tag.clone(),
        },
        entered,
    };
    for (own, avatar, mut player, fighting) in players {
        let seat = avatar.0;
        let leading = lent.filter(|_| seat == LEADER_SEAT);
        let who = &Id(acting_body(seat, own.0, lent.map(|(id, _)| id)));
        let ps = &mut player.0;
        // Where it uses from: its body, or the leading companion's.
        let user = leading.map_or(*ps, |(_, body)| body);
        // Use: a standing body's, unless it is spent on a revive (`falls`).
        let using = fighting.is_none_or(|fighting| fighting.0.alive())
            && reviving.0 & party::bit(seat) == 0
            && inputs
                .0
                .get(seat)
                .is_some_and(|command| command.buttons & buttons::USE != 0);
        if !using {
            ps.flags &= !motion::motion_flags::USE_SPENT;
        }
        // A held use acts once, as KOTOR's click does: what it used, talked
        // to or was refused by (a locked door) is not used again, nor
        // anything else, until use is let go. JA's use repeats while held
        // (`useDelay`), which ran a placeable's OnUsed twice for one press
        // (Manaan's flow button, its water sent on twice).
        let spent = ps.flags & motion::motion_flags::USE_SPENT != 0;
        // What the key used this step: a click on it this step is the same
        // use, not a second.
        let mut keyed: Option<u32> = None;
        if using && !spent && ps.next_use_ms < clock.time_ms {
            ps.next_use_ms = clock.time_ms + USE_EVERY_MS;
            let all: Vec<&Object> = objects.iter().map(|(_, object)| &**object).collect();
            let seated = seated.iter().map(|(id, player)| (*id, player));
            let watchable = watchable((&seats, lent_fighter), at_tables, (who.0, clock.time_ms));
            let met = with_tables(&talkers, seated, &watchable);
            let pick = key_use(&level.0, &all, &user, &met).filter(|pick| match *pick {
                KeyUse::Creature(_) => !unclickable(ObjectKind::Creature, Verb::Talk),
                KeyUse::Object(index) => find(&objects, index).is_none_or(|at| {
                    let object = &objects[at].1;
                    !unclickable(object.kind, uses::default_verb(object))
                }),
            });
            if let Some(KeyUse::Creature(creature)) = pick {
                ps.flags |= motion::motion_flags::USE_SPENT;
                used.0.push((creature, who.0));
            } else if let Some(KeyUse::Object(index)) = pick
                && let Some(at) = find(&objects, index)
            {
                let (id, object) = &mut objects[at];
                ps.flags |= motion::motion_flags::USE_SPENT;
                keyed = Some(object.index);
                let verb = uses::default_verb(object);
                let outcome = uses::use_object(
                    id.0,
                    object,
                    verb,
                    seat,
                    (who.0, scale(user.origin, UNIT)),
                    &mut uses::Events {
                        opened: &mut opened,
                        closed: &mut closed,
                        clicked: &mut clicked,
                        used: &mut used,
                    },
                );
                match outcome {
                    uses::Outcome::Leave(link) => asks.push(ask(id, &link, seat, None)),
                    uses::Outcome::Locked => opened.0.push((id.0, Some(who.0), false)),
                    uses::Outcome::Done => {}
                }
            }
        }
        // The seat's Uses, each checked first (`uses::may_use`) from the
        // body it acts with: the companion leading for the first seat. None
        // goes through while the world is held (above).
        let mut states = None;
        for (_, action) in inputs.0.actions().iter().filter(|(of, _)| *of == seat) {
            let Action::Use {
                object: index,
                verb,
            } = *action
            else {
                continue;
            };
            let Some(at) = find(&objects, index) else {
                continue;
            };
            if held || keyed == Some(index) {
                continue;
            }
            let Some(facts) = objects[at].1.facts.clone() else {
                continue;
            };
            let placed = facts.position;
            let creature = standing.iter().find(|(id, ..)| *id == objects[at].0.0);
            if !uses::applies(&objects[at].1, verb, creature.is_some_and(|c| c.3)) {
                continue;
            }
            let creature = creature.map(|(_, origin, ..)| scale(*origin, UNIT));
            let kind = objects[at].1.kind;
            let target = uses::Target::new(index, creature, placed).aimed(
                &level.0,
                kind,
                &facts,
                scale(user.origin, UNIT),
            );
            let states = states.get_or_insert_with(|| {
                level.0.states(
                    objects
                        .iter()
                        .map(|(_, object)| (object.index, &object.state)),
                )
            });
            if unclickable(kind, verb)
                || fighting.is_some_and(|fighting| !fighting.0.alive())
                || reviving.0 & party::bit(seat) != 0
                || !uses::may_use(&level.0, states.as_deref(), &user, &target, kind, verb)
            {
                continue;
            }
            let blocked = verb == Verb::Close
                && kind == ObjectKind::Door
                && standing
                    .iter()
                    .any(|(_, origin, maxs_z, _)| in_doorway(&level.0, index, *origin, *maxs_z));
            if blocked {
                continue;
            }
            // A click with use held is that press's use: the key's repeat
            // does not use again.
            if using {
                ps.flags |= motion::motion_flags::USE_SPENT;
            }
            let (id, object) = &mut objects[at];
            let outcome = uses::use_object(
                id.0,
                object,
                verb,
                seat,
                (who.0, scale(user.origin, UNIT)),
                &mut uses::Events {
                    opened: &mut opened,
                    closed: &mut closed,
                    clicked: &mut clicked,
                    used: &mut used,
                },
            );
            match outcome {
                uses::Outcome::Leave(link) => asks.push(ask(id, &link, seat, None)),
                uses::Outcome::Locked => opened.0.push((id.0, Some(who.0), false)),
                uses::Outcome::Done => {}
            }
        }
        // Doors run into.
        if let Some((_, touches)) = touched.0.iter().find(|(s, _)| *s == seat) {
            for &entity in touches {
                let Some(at) = find(&objects, entity) else {
                    continue;
                };
                let (id, object) = &objects[at];
                if object.kind != ObjectKind::Door {
                    continue;
                }
                let exit = object
                    .facts
                    .as_ref()
                    .and_then(|facts| facts.exit().filter(|_| !object.state.locked(facts.opens)));
                if let Some(link) = exit {
                    asks.push(ask(id, link, seat, None));
                }
            }
        }
        let bit = party::bit(seat);
        // A body down is in no doorway or trigger: it asks nothing and is
        // not at an exit; getting up where it lies enters it again.
        let standing = fighting.is_none_or(|fighting| fighting.0.alive());
        // An open door that leads out stops nothing, so walking into its
        // doorway is running into it: entering it asks, once, as entering
        // a trigger does; the seat is its tenant until it steps out. The
        // first seat enters with the companion it leads while one leads
        // (TSL's area transitions answer the party leader the player
        // controls: Bao-Dur leaving 233TEL after the pad's scene).
        for (id, object) in objects.iter_mut() {
            if object.kind != ObjectKind::Door {
                continue;
            }
            let Some(facts) = object.facts.clone() else {
                continue;
            };
            let Some(link) = facts.exit() else {
                continue;
            };
            let inside = standing
                && object.state.state != 0
                && in_doorway(&level.0, object.index, user.origin, user.maxs_z);
            let tenant = object.state.tenants & bit != 0;
            if inside && !tenant {
                object.state.tenants |= bit;
                asks.push(ask(id, link, seat, Some(object.index)));
            } else if !inside && tenant {
                object.state.tenants &= !bit;
            }
        }
        // Triggers.
        let body = scale(user.origin, UNIT);
        for (id, object) in objects.iter_mut() {
            if object.kind != ObjectKind::Trigger {
                continue;
            }
            let Some(facts) = object.facts.clone() else {
                continue;
            };
            let inside = standing && level.0.in_trigger(&facts, body);
            let tenant = object.state.tenants & bit != 0;
            if inside && !tenant {
                object.state.tenants |= bit;
                if let Some(link) = facts.exit() {
                    asks.push(ask(id, link, seat, Some(object.index)));
                }
            } else if !inside && tenant {
                object.state.tenants &= !bit;
            }
        }
    }
    asked.0 = asks;
}

/// What a use key pressed acts on (`key_use`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyUse {
    /// A creature, talked to.
    Creature(SimId),
    /// A module's object (its index): opened, used, looked in.
    Object(u32),
}

/// A use a step heard (`SimWorld::uses_heard`): the object's entity, the
/// body that used it, and what it came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UseHeard {
    pub object: SimId,
    pub by: SimId,
    pub came: UseCame,
}

/// What a use came to (`UseHeard`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseCame {
    /// Talked to or used: a creature's or placeable's scripts told, or a
    /// locked door's conversation asked for.
    Used,
    /// A door or container opened.
    Opened,
    /// A door or container that stayed shut: locked (its fail-to-open
    /// script told).
    Refused,
    /// An open door clicked (its click script told).
    Clicked,
}

/// The living creatures a use may talk to, their bodies ordered by entity
/// (`interact`): none in a fight (KOTOR's click on one attacks; a use would
/// start a hostile's conversation), and none of the party's, whose bodies
/// a use passes through (a companion beside the Ebon Hawk's ramp takes no
/// use meant for it), the companion leading (`lent`) among them.
pub(super) fn talkers<'a>(
    time_ms: u64,
    lent: Option<SimId>,
    creatures: impl Iterator<Item = (&'a Id, &'a Player, &'a Fighting)>,
) -> Vec<Body> {
    let mut talkers: Vec<Body> = creatures
        .filter(|(id, _, fighting)| {
            fighting.0.alive()
                && fighting.0.mind.leader.is_none()
                && fighting.0.combat_until <= time_ms
                && lent != Some(id.0)
        })
        .map(|(id, player, _)| body(id.0, &player.0))
        .collect();
    talkers.sort_by_key(|body| body.entity);
    talkers
}

/// The bodies a use may meet: `talkers`, and the bodies the seats act
/// through (`seated`: the companion leading too) playing a match the user
/// is free to watch (`watchable`), party members or not: its use of one
/// asks to watch that match (R9a, R36). For a user not free it passes
/// through them, as through the party's otherwise, to what is behind (the
/// dealer across the table).
pub(super) fn with_tables<'a>(
    talkers: &[Body],
    seated: impl Iterator<Item = (SimId, &'a MotionState)>,
    watchable: &[u32],
) -> Vec<Body> {
    let mut met = talkers.to_vec();
    met.extend(
        seated
            .filter(|(id, _)| watchable.contains(&id.0))
            .map(|(id, player)| body(id, player)),
    );
    met.sort_by_key(|body| body.entity);
    met
}

/// The players whose match `user` (a seat's acting body) is free to watch
/// (R9a, R41: `script::WatchRule::free`, the scripts' one rule), read off
/// this step's resources as a client predicting reads them too: the seats'
/// own bodies and fighters (`seats`), the companion the host leads through
/// (`lent`), the conversations, the trades, the matches and who a fight is
/// near (`party::Embattled`, written earlier in the step).
#[allow(clippy::type_complexity)]
pub(super) fn watchable(
    (seats, lent): (
        &[(Seat, SimId, Option<&Fighter>)],
        Option<(SimId, &Fighter)>,
    ),
    (talking, goods, tables, embattled): (
        &Told,
        &crate::inventory::Goods,
        &[crate::pazaak::Match],
        &[SimId],
    ),
    (user, now): (SimId, u64),
) -> Vec<u32> {
    use script_ncs::vm::ObjectId;
    if tables.is_empty() {
        return Vec::new();
    }
    let lent_id = lent.map(|(id, _)| id);
    let acting: Vec<(u8, ObjectId)> = (seats.iter())
        .map(|&(seat, id, _)| (seat.0, ObjectId(acting_body(seat, id, lent_id).0)))
        .collect();
    let seat_0 = (seats.iter()).find(|(seat, ..)| *seat == LEADER_SEAT);
    let embattled: Vec<ObjectId> = embattled.iter().map(|id| ObjectId(id.0)).collect();
    let rule = crate::script::WatchRule {
        seats: &acting,
        host: host_bodies(seat_0.map(|(_, id, _)| *id), lent_id),
        embattled: &embattled,
        conversations: &talking.conversations,
        tables,
    };
    let fighter = match lent.filter(|(id, _)| *id == user) {
        Some((_, fighter)) => Some(fighter),
        None => (seats.iter())
            .find(|(_, id, _)| *id == user)
            .and_then(|(.., fighter)| *fighter),
    };
    let who = crate::script::Watcher {
        body: ObjectId(user.0),
        down: fighter.is_none_or(Fighter::downed),
        in_combat: fighter.is_some_and(|fighter| fighter.combat_until > now),
        trading: goods.trade(user.0).is_some(),
    };
    (tables.iter())
        .map(|table| table.player)
        .filter(|player| rule.free(who, None, ObjectId(*player)))
        .collect()
}

/// What `user`'s use key, pressed now, acts on, `objects` ordered by index:
/// the first creature (`talkers`) or object its look meets within reach
/// (a creature at the talk distance, anything else at the use distance),
/// else a container the look cannot meet (no walkmesh: remains), or, when
/// it met nothing, a usable placeable that speaks or has a use script (a
/// terminal; the Vulkars' accelerator on the floor) within reach in height
/// too: the nearest in front of the user.
pub(super) fn key_use(
    level: &Level,
    objects: &[&Object],
    user: &MotionState,
    talkers: &[Body],
) -> Option<KeyUse> {
    let states = level.states(objects.iter().map(|object| (object.index, &object.state)));
    let mut eye = user.origin;
    eye[2] += user.eye_height;
    let (forward, _, _) = math::angle_vectors(user.view);
    let reach = math::mad(eye, TALK_DISTANCE, forward);
    let mut trace = level.with(states.as_deref(), |world| {
        Bodies::new(world, talkers).trace(
            scale(eye, UNIT),
            scale(reach, UNIT),
            &Extents::POINT,
            Contents::SOLID | Contents::BODY,
        )
    });
    let body = trace.entity.filter(|entity| entity & BODY != 0);
    let within = USE_DISTANCE / TALK_DISTANCE;
    // A creature past the use distance gives way to a usable placeable
    // before it (Davik's ramp, invisible, a companion behind it); talked to
    // when there is none.
    let far = body.is_some() && trace.fraction > within;
    // Anything but a creature is reached at the use distance only.
    if body.is_none() {
        if trace.fraction > within {
            trace.fraction = 1.0;
            trace.entity = None;
        } else {
            trace.fraction /= within;
        }
    }
    let creature = |entity: u32| KeyUse::Creature(SimId(entity & !BODY));
    if trace.fraction < 1.0 && !far {
        if let Some(entity) = body {
            return Some(creature(entity));
        }
        if let Some(entity) = trace.entity
            && objects
                .binary_search_by_key(&entity, |object| object.index)
                .is_ok()
        {
            return Some(KeyUse::Object(entity));
        }
    }
    let me = scale(user.origin, UNIT);
    // How far the look went before a wall (metres): a placeable before it is
    // not behind it (Davik's ramp, invisible, before the Ebon Hawk's hull).
    // The floor looked down at is no wall.
    let floor = trace.plane.normal[2] > motion::WALKABLE_NORMAL_Z;
    let open = if far || floor {
        f32::MAX
    } else {
        trace.fraction * USE_DISTANCE * UNIT
    };
    objects
        .iter()
        .filter_map(|object| {
            let facts = object.facts.as_ref()?;
            let distance = containers::ahead(me, [forward[0], forward[1]], facts.position)?;
            // (Its middle may stand a little inside the wall it is fixed
            // to.)
            let speaks = (trace.fraction >= 1.0 || distance <= open + USE_SLACK_M)
                && object.kind == ObjectKind::Placeable
                && facts.useable
                && crate::script::hooks::speaks(facts)
                && (facts.position[2] - me[2]).abs() <= uses::HEIGHT_REACH_M;
            (facts.container || speaks).then_some((distance, object.index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| KeyUse::Object(index))
        .or_else(|| body.filter(|_| far).map(creature))
}

/// Whether a player at `origin` (units) standing `maxs_z` tall is in the
/// doorway of door `index`: its box, a unit larger all round, overlaps the
/// door's closed shape (the door's own brushes, whatever else the box
/// touches).
pub(crate) fn in_doorway(level: &Level, index: u32, origin: [f32; 3], maxs_z: f32) -> bool {
    near_door(level, index, origin, maxs_z, 1.0)
}

/// Whether a player at `origin` (units) standing `maxs_z` tall is within
/// `by` units of door `index`: its box, that much larger all round,
/// overlaps the door's closed shape.
pub(crate) fn near_door(level: &Level, index: u32, origin: [f32; 3], maxs_z: f32, by: f32) -> bool {
    let Some(built) = level.collision.level() else {
        return false;
    };
    let grown = |v: f32, by: f32| (v + by) * UNIT;
    let extents = Extents {
        mins: [
            grown(-motion::HALF_WIDTH, -by),
            grown(-motion::HALF_WIDTH, -by),
            grown(motion::SOLE_Z, -by),
        ],
        maxs: [
            grown(motion::HALF_WIDTH, by),
            grown(motion::HALF_WIDTH, by),
            grown(maxs_z, by),
        ],
    };
    built.entity_overlaps(index, 0, scale(origin, UNIT), &extents, Contents::SOLID)
}
