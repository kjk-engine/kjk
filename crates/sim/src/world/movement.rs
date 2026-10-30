//! The step's moves: players held in conversations, then every actor
//! moved (`move_actors`).

use super::*;

/// A player held in a conversation stands still and does nothing else:
/// its command keeps where it looks, its selections and its answer, its
/// yaw held where the body faces (a line's start turns it, S2).
///
/// A seat's body is held in the conversation it stands in (`Told::holding`,
/// R1), else, while a party conversation is up (`Told::party`), in that
/// one: every seat's body then. A body watching a pazaak match (`Tabled`,
/// R9a) is held so too, turned to the table's player, the match paused or
/// not: its table stays up with Leave (R39), and a fight that reaches it
/// ends its watching. A watching seat has no body, and keeps its command.
pub(super) fn hold_talkers(
    (talking, tabled): (Res<Talking>, Res<Tabled>),
    mut inputs: ResMut<Inputs>,
    avatars: Query<(&Id, &Avatar, &Player)>,
    others: Query<(&Id, Option<&Player>, Option<&Placed>)>,
    scripts: Res<scripts::Scripts>,
) {
    let watched = tabled.0.iter().any(|table| !table.watchers.is_empty());
    if talking.0.conversations.is_empty() && !watched {
        return;
    }
    let lent = lent(&scripts.state.party);
    let party = talking.0.party();
    for (id, avatar, player) in &avatars {
        let acting = acting_body(avatar.0, id.0, lent);
        // A conversation with the companion it moves holds it too.
        let id = &match lent.filter(|_| avatar.0 == LEADER_SEAT) {
            Some(body)
                if talking
                    .0
                    .holding(script_ncs::vm::ObjectId(body.0))
                    .is_some() =>
            {
                Id(body)
            }
            _ => Id(id.0),
        };
        let own = talking.0.holding(script_ncs::vm::ObjectId(id.0.0));
        let table = tabled
            .0
            .iter()
            .find(|table| table.watchers.contains(&acting.0));
        let faced = match (own.or(party), table) {
            (Some(_), _) => None,
            (None, Some(table)) => Some(table.player),
            (None, None) => continue,
        };
        let Some(command) = inputs.0.get(avatar.0).copied() else {
            continue;
        };
        let table_player =
            faced.and_then(|faced| others.iter().find(|(other, ..)| other.0.0 == faced));
        let toward = table_player.and_then(|(_, body, placed)| match (body, placed) {
            (Some(body), _) => Some(body.0.origin),
            (None, Some(placed)) => Some(scale(placed.0, 1.0 / UNIT)),
            _ => None,
        });
        let from = player.0.origin;
        let yaw = toward
            .filter(|_| faced != Some(id.0.0))
            .map_or(player.0.view[1], |to| {
                math::atan2_deg(to[1] - from[1], to[0] - from[0])
            });
        let mut angles = command.angles;
        angles[1] = mind::looking(&player.0, 0.0, yaw).angles[1];
        let held = Command {
            angles,
            reply: command.reply,
            weapon: command.weapon,
            force: command.force,
            ..Command::default()
        };
        inputs.0.insert(avatar.0, held);
    }
}

/// Characters in order, each moved by its command through move_body: players
/// in seat order, then creatures in id order (`command_for`). Doors stand as
/// their states have them, and every living character is a body others
/// run into, moved before the next character's move (as JA links each
/// client after its move) — except that party members pass through each
/// other with friendly fire off (a delegated decision: JA's players all
/// collide, which in co-op KOTOR's doorways would only get in the way). The
/// dead and the downed meet only the level and what play made (`lying`).
///
/// Each character's move runs its weapon step too (`motion::Arms`): its
/// weapon changes, knockdowns and saber moves, whose checks see the other
/// living characters as they stand when it moves, and whose synced rolls
/// (JA's time-seeded synced roll) come from a hash of the tick
/// and the character (`SimRng::new` as the hash), so no one's rolls depend
/// on who else moved.
///
/// Predicting, a creature that is dead or farther than `PREDICT_NEAR` from
/// every player keeps its state: nothing it does there can meet a player
/// before the next authority snapshot corrects it, and its moves are most
/// of what re-stepping costs (the authority moves everyone).
#[allow(clippy::type_complexity)]
pub(super) fn move_actors(
    clock: Res<Clock>,
    inputs: Res<Inputs>,
    level: Res<Static>,
    (mut touched, mut bumped, mut sent): (ResMut<Touched>, ResMut<Bumped>, ResMut<TorsoSent>),
    (driven, scripts, held, talking, fight, made_faces): (
        Res<Driven>,
        Res<scripts::Scripts>,
        Res<scene::Held>,
        Res<Talking>,
        Res<InCombat>,
        Res<MadeFaces>,
    ),
    (objects, made): (
        Query<&Object>,
        Query<(&Id, &Object, &Placed), Without<Player>>,
    ),
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Player,
        &mut Fighting,
    )>,
) {
    let states = door_states(&level.0, &objects);
    let lent = lent(&scripts.state.party);
    sent.0 = actors
        .iter()
        .map(|(id, .., player, _)| (id.0, player.0.torso.anim))
        .collect();
    let mut actors: Vec<_> = actors.iter_mut().collect();
    actors.sort_by_key(|(id, avatar, ..)| (avatar.is_none(), avatar.map(|a| a.0), id.0));
    let living = |party: bool| {
        let mut bodies: Vec<Body> = actors
            .iter()
            .filter(|(_, avatar, .., fighting)| {
                fighting.0.alive() && (!party || !in_party(*avatar, &fighting.0))
            })
            .map(|(id, _, object, _, player, _)| blocking(id.0, &player.0, *object))
            .collect();
        bodies.sort_by_key(|body| body.entity);
        bodies
    };
    // What the rest's moves meet, and what the party's do; both meet what
    // play made.
    let placed = level.0.placed().len();
    let mut props: Vec<Body> = made
        .iter()
        .flat_map(|(id, object, at)| made_bodies(id.0, object, at.0, placed))
        .collect();
    props.sort_by_key(|body| body.entity);
    // Where each one stands, for a body inside its footprint (made on top
    // of it: Calo Nord's rubble falls where he surrenders), whose edge
    // cells hold nothing inside.
    let footprints: Vec<Footprint> = made
        .iter()
        .filter_map(|(id, object, at)| made_footprint(id.0, object, at.0, placed))
        .collect();
    let (mut everyone, mut outside) = (living(false), living(true));
    for bodies in [&mut everyone, &mut outside] {
        bodies.extend(props.iter().copied());
        bodies.sort_by_key(|body| body.entity);
    }
    // What the dead's moves meet: what play made alone. A corpse passes
    // through bodies (JA's `PmoveSingle` takes `CONTENTS_BODY` out of the
    // mask of one with no health left), so the living walking through it
    // do not shove it along: a Sith thug's on Korriban, run through by the
    // player, was pushed 7 m and its remains stood far from where it fell.
    let lying = &props;
    let mut others: Vec<motion::Other> = actors
        .iter()
        .filter(|(.., fighting)| fighting.0.alive())
        .map(|(id, _, _, _, player, _)| motion::Other {
            entity: id.0.0 | BODY,
            legs: player.0.legs.anim,
        })
        .collect();
    others.sort_by_key(|other| other.entity);
    // Each living character's eye, by id: a creature's swings aim at its
    // enemy's.
    let mut eyes: Vec<(SimId, [f32; 3])> = actors
        .iter()
        .filter(|(.., fighting)| fighting.0.alive())
        .map(|(id, _, _, _, player, _)| (id.0, crate::combat::mind::eye(&player.0)))
        .collect();
    eyes.sort_by_key(|(id, _)| *id);
    let players: Vec<[f32; 3]> = actors
        .iter()
        .filter(|(_, avatar, ..)| avatar.is_some())
        .map(|(.., player, _)| player.0.origin)
        .collect();
    // Who KOTOR's walkers push aside rather than bump into (`walk::Met`),
    // by id: a creature idle (nothing queued, `Mind::queued`; no walk, no
    // animation playing now, a scene's clip played out counting as none
    // (Mission, after her scene in Javyar's Cantina, was never pushed aside
    // and the Vulkars leaving bumped into her until they gave up), out of a
    // fight and of conversations; not a player
    // nor the party's, nor one that never moves (`Body::immobile`: Telos's
    // hidden turret in Slusk's office was pushed 0.9 m into the way out)),
    // with its side; and everyone's personal space, and
    // where a walk not given up carries it (`walk::Met::going`). All of it
    // is in the snapshot, so a client predicts the pushes the authority
    // makes.
    let mut pushable: Vec<(SimId, Option<Side>, f32, Option<[f32; 3]>)> = actors
        .iter()
        .filter(|(.., fighting)| fighting.0.alive())
        .map(|(id, avatar, object, sheet, _, fighting)| {
            let fighter = &fighting.0;
            let who = id.0.0;
            let idle = avatar.is_none()
                && object.is_some()
                && !in_party(*avatar, fighter)
                && fighter.mind.walker.is_none()
                && fighter.mind.task == crate::combat::ai::Task::Idle
                && !fighter
                    .acting
                    .is_some_and(|acting| acting.playing(clock.time_ms))
                && fighter.combat_until <= clock.time_ms
                && !fighter.mind.queued
                && talking.0.holding(script_ncs::vm::ObjectId(who)).is_none()
                && !object
                    .and_then(|object| object.facts.as_ref()?.body)
                    .is_some_and(|body| body.immobile());
            let side = idle.then(|| side_of(id.0, *avatar, fighter, &sheet_of(*sheet, *object)));
            let going = fighter
                .mind
                .walker
                .filter(|walker| !walker.stuck())
                .map(|walker| walker.heading());
            (id.0, side, personal_space(*object), going)
        })
        .collect();
    pushable.sort_by_key(|(id, ..)| *id);
    let mut pushes: Vec<(SimId, [f32; 3])> = Vec::new();
    let predicting = clock.reason == StepReason::Predict;
    // While a conversation holds the world (S3) a body walking a route
    // moves on KOTOR's walker (`walk::kotor_step`), at its
    // `creaturespeed.2da` rates: a seat's body at the player's.
    let held = held.world;
    // The bodies the seats move, which walk at the `PLAYER` row's rates.
    let seat_bodies = scene::seat_bodies(
        actors
            .iter()
            .filter_map(|(id, avatar, ..)| avatar.map(|avatar| (avatar.0, id.0))),
        lent,
    );
    let table = move_rates(&level.0);
    // Each object's state by its placement, for the doors KOTOR's walker
    // meets.
    let object_states: std::collections::BTreeMap<u32, crate::snapshot::ObjectState> = objects
        .iter()
        .map(|object| (object.index, object.state))
        .collect();
    let object_state = |index: u32| {
        object_states
            .get(&index)
            .copied()
            .unwrap_or(crate::snapshot::ObjectState::GONE)
    };
    touched.0.clear();
    bumped.0.clear();
    for (id, avatar, object, sheet, player, fighting) in actors.iter_mut() {
        let (id, avatar, object, sheet) = (*id, *avatar, *object, *sheet);
        let placed_object = object;
        if predicting && avatar.is_none() {
            let near = players.iter().any(|at| {
                let d = math::sub(*at, player.0.origin);
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= PREDICT_NEAR * PREDICT_NEAR
            });
            if !fighting.0.alive() || !near {
                continue;
            }
        }
        let scripted = driven.0.contains(&id.0);
        // A scene's attacker whose blow begins (`CutsceneAttack`) plays it
        // at once, up from a knockdown (a script's push): KOTOR's attack
        // takes over from what its body played, as its move does
        // [verify: the K1 exe's attack action on a pushed creature].
        let begins = |strike: &crate::script::effects::Strike| {
            strike.by.0 == id.0.0
                && (strike.starts_ms()..=strike.starts_ms() + 2 * u64::from(clock.msec))
                    .contains(&clock.time_ms)
        };
        if fighting.0.saber.floored()
            && fighting.0.alive()
            && scripts.state.strikes.iter().any(begins)
        {
            fighting.0.saber.stand_up();
        }
        // The companion leading takes its player's commands; the player's
        // own body stands meanwhile.
        let leads = lent
            .filter(|body| *body == id.0)
            .map(|_| Avatar(LEADER_SEAT));
        let stands = avatar.is_some_and(|avatar| avatar.0 == LEADER_SEAT) && lent.is_some();
        let seat = avatar.or(leads.as_ref()).filter(|_| !scripted && !stands);
        // Its seat's holster key puts its weapon away (`Sheath::Holstering`,
        // its weapon step plays it).
        if let Some(seat) = seat
            && fighting.0.alive()
            && fighting.0.sheath != Sheath::Holstered
            && inputs.0.actions().contains(&(seat.0, Action::Holster))
        {
            fighting.0.sheath = Sheath::Holstering;
        }
        // A route its mind walks: a creature's, a player's a script walks, a
        // guest's companion while its mind steers it, the companion leading
        // a script walks. Its command is the walker's, which the snapshot
        // carries, so a client predicting steps it as the authority does.
        let routed = fighting.0.mind.walker.is_some()
            && (seat.is_none()
                || fighting.0.mind.steered
                || leads.is_some()
                || (avatar.is_some() && object.is_none()));
        // Its `creaturespeed.2da` rates (`walk::rates`): a seat's body's the
        // table's `PLAYER` row.
        let seat_body = seat_bodies.contains(&script_ncs::vm::ObjectId(id.0.0));
        let rates = || {
            let body = object
                .and_then(|object| object.facts.as_ref())
                .and_then(|facts| facts.body.as_ref());
            walk::rates(body, seat_body || object.is_none(), table)
        };
        let ps = &player.0;
        // A script's move of a body no seat drives, out of a fight, that
        // stalled on move_body (a second with no headway: the level in its way,
        // as move_body meets bodies at once) goes on KOTOR's walker to its next
        // corner, where the walk plans on through move_body again. KOTOR's
        // walker is a point on the walkmesh and meets nothing drawn: a
        // tent's guy rope across the Outcast village's walkway (Taris) held
        // an outcast's box there until its forced move's timeout.
        let mind = &fighting.0.mind;
        let stalled_out = seat.is_none()
            && mind.moving
            && mind.task != crate::combat::ai::Task::Engage
            && mind
                .walker
                .is_some_and(|walker| walker.stalls > 0 && !walker.stuck());
        let kotor = ((held || stalled_out)
            && routed
            && fighting.0.alive()
            && !fighting.0.force.stunned(clock.time_ms)
            && ps.ground.is_some()
            && ps.flags & (motion::motion_flags::KNOCKBACK_HOLD | motion::motion_flags::DEAD) == 0)
            .then(rates)
            .flatten();
        let mut command = if kotor.is_some() {
            fighting.0.last_command
        } else if stands {
            Command {
                angles: player.0.command_angles,
                ..Command::default()
            }
        } else {
            command_for(&inputs.0, seat, &player.0, &fighting.0, clock.time_ms)
        };
        // A creature its appearance gives no movement (`moverate` NOMOVE,
        // both rates none: a turret) moves not at all, in a fight too (JA's
        // pace there let Telos's hidden turrets chase the Exile out of their
        // alcove into the Exchange's doorway); it turns and attacks.
        if seat.is_none() && rates().is_some_and(|(walk, run)| walk <= 0.0 && run <= 0.0) {
            command.forward = 0;
            command.right = 0;
            command.up = 0;
        }
        // KOTOR's stealth disables the run (`world::covert`), as a worn
        // item's Special Walk does: its axes held to a walk's, as JA's client
        // sends a walk.
        let walk_only = fighting.0.effects.worn_traits & gear::traits::WALK_ONLY != 0;
        if fighting.0.covert.stealth || walk_only {
            let walk = rules::stealth::mode::WALK_AXIS;
            command.forward = command.forward.clamp(-walk, walk);
            command.right = command.right.clamp(-walk, walk);
            command.buttons |= buttons::WALKING;
        }
        // Out of a fight a body moves at KOTOR's pace (`paced`): its
        // `creaturespeed.2da` walk or run, a seat's body's the table's
        // `PLAYER` row, as KOTOR moves everyone; a fight keeps JA's.
        let party = in_party(avatar, &fighting.0);
        player.0.flags &= !motion::motion_flags::PACED;
        let mut paced_base = None;
        if kotor.is_none() && fighting.0.alive() {
            let mind = &fighting.0.mind;
            let scripted_move =
                seat.is_none() && mind.task != crate::combat::ai::Task::Engage && mind.moving;
            // A seat's body, and a companion following it, keep JA's in
            // the party's fight or with weapons out (JA's gaits then).
            let as_players = seat.is_some() || mind.task == crate::combat::ai::Task::Follow;
            let fights = !fighting.0.weapon_away(clock.time_ms) || (party && fight.0);
            let rates = (scripted_move || (as_players && !fights))
                .then(rates)
                .flatten();
            if let Some(rates) = rates {
                let run = command.buttons & buttons::WALKING == 0;
                let speed = walk::kotor_speed(rates, player.0.run_speed / motion::BASE_SPEED, run);
                // Out of its fight lately, it eases down from JA's.
                let left = fighting.0.combat_until.max(fighting.0.saber.out_until);
                let keep = match left {
                    0 => 0.0,
                    _ if as_players => walk::pace_kept(clock.time_ms.saturating_sub(left)),
                    _ => 0.0,
                };
                paced_base = Some(walk::paced(&command, &player.0, speed, keep));
                player.0.flags |= motion::motion_flags::PACED;
                if let Some(walker) = fighting.0.mind.walker.as_mut() {
                    walker.give_up_unable(&command, speed);
                }
            }
        }
        fighting.0.last_command = command;
        let own = id.0.0 | BODY;
        // With friendly fire on (a duel), the party collides too.
        let passes = level.0.friendly_fire == FriendlyFire::Off;
        let bodies = if !fighting.0.alive() {
            lying
        } else if passes && party {
            &outside
        } else {
            &everyone
        };
        // The bodies its walk may run into; a creature walking pushes aside
        // the idle ones not hostile to it, the party leader none (KOTOR's).
        let me = routed.then(|| side_of(id.0, avatar, &fighting.0, &sheet_of(sheet, object)));
        let pusher = avatar.is_none() && leads.is_none();
        let space = personal_space(object);
        let mets: Vec<walk::Met> = bodies
            .iter()
            .filter(|_| routed)
            .filter(|body| body.entity & BODY != 0 && body.entity != own)
            .filter_map(|body| {
                let who = SimId(body.entity & !BODY);
                let at = pushable.binary_search_by_key(&who, |(id, ..)| *id).ok()?;
                let (_, side, theirs, going) = pushable[at];
                let push = side
                    .zip(me)
                    .filter(|(side, me)| {
                        pusher && crate::module::relation(None, *me, *side) != Relation::Hostile
                    })
                    .map(|_| theirs);
                Some(walk::Met {
                    who,
                    at: origin_of(body),
                    push,
                    going,
                })
            })
            .collect();
        let stepped = kotor.and_then(|rates| {
            let walker = fighting.0.mind.walker.as_mut()?;
            let moves = command.forward != 0 || command.right != 0;
            let run = command.buttons & buttons::WALKING == 0;
            let multiplier = player.0.run_speed / motion::BASE_SPEED;
            let speed = if moves {
                walk::kotor_speed(rates, multiplier, run)
            } else {
                0.0
            };
            walker.give_up_unable(&command, speed);
            // The doors as this body may open them: none, for the party
            // while the world is held (S4).
            let doors = |index: u32| match object_state(index) {
                state if party && held => walk::opening_none(state),
                state => state,
            };
            let ways = walk::Walled {
                doors: &doors,
                walled: &made_faces.0,
            };
            walk::kotor_step(
                &level.0, &player.0, walker, speed, clock.msec, run, space, &mets, &ways,
            )
        });
        let kit = Kit::of(&sheet_of(sheet, object));
        let mut rolls = SimRng::new(clock.tick, u64::from(id.0.0));
        let enemy = fighting
            .0
            .target
            .and_then(|target| eyes.binary_search_by_key(&target, |(id, _)| *id).ok())
            .map(|at| eyes[at].1);
        let mut arms = motion::Arms {
            fighter: &mut fighting.0,
            kit: &kit,
            player: avatar.is_some(),
            others: &others,
            rng: &mut rolls,
            enemy,
        };
        // A made box it stands in (made where it stood) holds it not: it
        // walks out of it.
        let mine = body(id.0, &player.0);
        // A seat's player meets a body it touches as one it stands inside:
        // it slides along it, as K1's player slides along a creature
        // (Q-0267), rather than being held by its box's corner. In
        // Chuundar's throne room Freyyr, come up beside the player at a
        // slanting wall, held it with his box's corner on either side, a
        // pocket no walk got out of.
        let inside = clear_of(&met_as(&mine, avatar.is_some()), bodies);
        let bodies = inside.bodies.as_ref();
        let before = player.0.origin;
        let standing_in = props
            .iter()
            .find(|prop| {
                (0..3)
                    .all(|axis| prop.min[axis] < mine.max[axis] && mine.min[axis] < prop.max[axis])
            })
            .map(|prop| prop.entity)
            .or_else(|| {
                let at = scale(player.0.origin, UNIT);
                footprints
                    .iter()
                    .find(|footprint| footprint.holds(at))
                    .map(|footprint| footprint.entity)
            });
        // KOTOR has no falling: a creature with no floor at all below it
        // (placed over a room the game gives no walkable face) stands where
        // it is, as the reference keeps it, rather than falling forever.
        // Its feet's box is what finds the floor, as its fall would: a
        // point slips through a crack between rocks (Thalia, out of a jump
        // in Korriban's Shyrack cave, stood frozen in the air, parrying
        // every blow). Feet that start in the level touch it, and stand on
        // it: the trace passes out through what it starts in (Jolee, placed
        // with his feet on Kashyyyk's Shadowlands floor, stood rooted while
        // the katarns his scene waits on ran round him).
        let floorless = avatar.is_none()
            && player.0.ground.is_none()
            && level.0.with(states.as_deref(), |world| {
                let from = scale(player.0.origin, UNIT);
                let below = [from[0], from[1], from[2] - FLOOR_SEARCH_M];
                let half = motion::HALF_WIDTH * UNIT;
                let feet = Extents {
                    mins: [-half, -half, motion::SOLE_Z * UNIT],
                    maxs: [half, half, 0.0],
                };
                let trace =
                    world.trace(from, below, &feet, Contents::SOLID | Contents::PLAYER_CLIP);
                !trace.start_solid && trace.fraction >= 1.0
            });
        // One KOTOR's walker moved last step and no longer walking stops
        // dead, as KOTOR's bodies carry no momentum.
        if stepped.is_none() && player.0.flags & motion::motion_flags::CARRIED != 0 {
            player.0.flags &= !motion::motion_flags::CARRIED;
            if command.forward == 0 && command.right == 0 {
                player.0.velocity = [0.0; 3];
            }
        }
        // A paced step moves at its pace's base speed; the body's own (its
        // effects' and powers') is kept.
        let run_speed = player.0.run_speed;
        if let Some(paced) = paced_base {
            player.0.run_speed = paced;
        }
        let touches = if let Some(stepped) = &stepped {
            level.0.with(states.as_deref(), |world| {
                let world = Bodies::new(world, bodies).skipping(own);
                motion::carry(
                    &mut player.0,
                    &command,
                    clock.msec,
                    clock.time_ms,
                    &world,
                    &level.0.anims,
                    Some(&mut arms),
                    stepped,
                );
            });
            // A shut door it stopped at is run into, as a move_body walker runs
            // into one (a creature opens it, `interact`).
            stepped.door.into_iter().collect()
        } else if floorless {
            Vec::new()
        } else {
            level.0.with(states.as_deref(), |world| {
                let world = Bodies::new(world, bodies).skipping(own);
                let world = match standing_in {
                    Some(entity) => world.skipping(entity),
                    None => world,
                };
                motion::move_body(
                    &mut player.0,
                    &command,
                    clock.msec,
                    clock.time_ms,
                    &world,
                    &level.0.anims,
                    Some(&mut arms),
                )
            })
        };
        player.0.run_speed = run_speed;
        // KOTOR's walker meets the bodies by its own rule (`kotor_step`: it
        // pushes one aside or bumps and goes round, passing the one it goes
        // round); held off as well, a body going round one it overlapped
        // stopped and started step by step (Dantooine's enclave droid).
        // A seat's player walks along a body it stands inside; a creature
        // steps back from it as before (sliding, Korriban's academy guards
        // moved others' ways and a player's walk turned back and forth).
        if stepped.is_none() {
            let maxs_z = player.0.maxs_z;
            let seated = avatar.is_some();
            inside.hold_off(before, &mut player.0, |from, to| {
                seated
                    && level.0.with(states.as_deref(), |world| {
                        let world = Bodies::new(world, bodies).skipping(own);
                        standing_clear(&world, from, to, maxs_z)
                    })
            });
        }
        // A body its walk carries on plays its walk or run: KOTOR's movement
        // replaces what a script or a line played on it, as reone's creature
        // takes its move clip and plays no other while it moves. A death's
        // clip and a scene's blow play on.
        if routed
            && fighting.0.alive()
            && math::flat_distance(before, player.0.origin) > WALKED
            && fighting.0.acting.is_some_and(|acting| {
                acting.playing(clock.time_ms)
                    && acting.animation != death::DIE
                    && crate::script::effects::blows::of(acting.animation).is_none()
            })
        {
            fighting.0.acting = None;
        }
        // A creature's walk on move_body that ran into a body meets it at once as
        // KOTOR's walker does (`Walker::meet`): pushes it aside, or bumps
        // into it and goes round, rather than stalling a second first. One
        // KOTOR's walker carried met what it ran into in its step.
        let scripted_move = seat.is_none()
            && fighting.0.mind.moving
            && fighting.0.mind.task != crate::combat::ai::Task::Engage;
        if let Some(stepped) = &stepped {
            pushes.extend(stepped.pushed);
        } else if routed
            && fighting.0.alive()
            && let Some(walker) = fighting.0.mind.walker.as_mut()
        {
            let at = player.0.origin;
            let toward = walker.heading();
            let ahead = |p: [f32; 3]| {
                (p[0] - at[0]) * (toward[0] - at[0]) + (p[1] - at[1]) * (toward[1] - at[1]) > 0.0
            };
            let met = touches
                .iter()
                .filter(|entity| *entity & BODY != 0)
                .filter_map(|entity| mets.iter().find(|met| met.who.0 == entity & !BODY))
                .find(|met| !walker.passes(met) && !walker.waits_behind(at, met) && ahead(met.at));
            if let Some(met) = met
                && let Some(spot) = walker.meet(&level.0, &player.0, space, met, &mets)
            {
                pushes.push((met.who, spot));
            }
            // A script's move out of a fight whose step went nowhere against
            // the level (a placeable it touched, or the level's own solid on
            // its way ahead, `walk::way_clear`; not a body nor a door, which
            // keep their own rules) has stalled: it goes on KOTOR's walker
            // from the next step (`stalled_out`), as after a second with no
            // headway. The mesh planned the leg, and KOTOR's walker, a point
            // on the walkmesh, walks it: Peragus's Sith troopers boarding the
            // Ebon Hawk (107PER), routed between an explosive barrel and a
            // crate's walkmesh edge their boxes do not fit between, stood a
            // second at each such gap, twenty times in the turret game.
            let commanded = command.forward != 0 || command.right != 0;
            let placeable = |entity: &u32| {
                level
                    .0
                    .placed()
                    .get(*entity as usize)
                    .is_some_and(|object| object.kind == crate::module::ObjectKind::Placeable)
            };
            if scripted_move
                && commanded
                && walker.stalls == 0
                && walker.waypoint.is_some()
                && math::flat_distance(before, player.0.origin) <= WALKED
                && !touches.iter().any(|entity| *entity & BODY != 0)
                && (touches.iter().any(placeable)
                    || !walk::way_clear(&level.0, &player.0, command.forward, command.right))
            {
                walker.stalls = 1;
            }
        }
        idle::check(
            &mut player.0,
            &mut fighting.0,
            (&command, avatar.is_some()),
            &kit,
            &level.0.anims,
            (clock.time_ms, clock.msec),
            &mut rolls,
        );
        for bodies in [&mut everyone, &mut outside] {
            if let Ok(at) = bodies.binary_search_by_key(&own, |body| body.entity) {
                bodies[at] = blocking(id.0, &player.0, placed_object);
            }
        }
        if let Ok(at) = others.binary_search_by_key(&own, |other| other.entity) {
            others[at].legs = player.0.legs.anim;
        }
        // Swinging shows a mind-tricking character again, as firing does
        // (JA's saber attack event).
        if fighting.0.saber.current_move.attack() {
            fighting.0.force.tricked = 0;
        }
        // A creature opens what it runs into only as `opens_doors` has it.
        // What a player's commands moved touched it: the leading
        // companion's body's, not its own standing one's.
        let seat = match (avatar, leads) {
            (Some(avatar), _) if !(avatar.0 == LEADER_SEAT && lent.is_some()) => Some(avatar.0),
            (_, Some(leads)) => Some(leads.0),
            _ => None,
        };
        match seat {
            Some(seat) => touched.0.push((seat, touches)),
            None if avatar.is_some() => {}
            None if opens_doors(&fighting.0) => {
                bumped
                    .0
                    .extend(touches.into_iter().map(|entity| (id.0, entity)));
            }
            None => {}
        }
    }
    // Those pushed aside stand where they were put, still; one pushed by
    // two walkers at once where the first put it.
    let mut moved: Vec<SimId> = Vec::new();
    for (who, spot) in pushes {
        if moved.contains(&who) {
            continue;
        }
        moved.push(who);
        if let Some((.., player, _)) = actors.iter_mut().find(|(id, ..)| id.0 == who) {
            player.0.origin = spot;
            player.0.velocity = [0.0; 3];
        }
    }
}

/// The bodies a move meets (`clear_of`): those given less the living ones
/// the mover already stands inside, and those ones' centres (units, seen
/// from above).
pub(crate) struct Inside<'b> {
    pub bodies: std::borrow::Cow<'b, [Body]>,
    centres: Vec<[f32; 2]>,
}

/// A body's round seen from above (units): its box's middle, and half its
/// width, as a body moving round across meets it (`collision::Bodies`).
fn round_of(body: &Body) -> ([f32; 2], f32) {
    (
        [
            (body.min[0] + body.max[0]) * 0.5 / UNIT,
            (body.min[1] + body.max[1]) * 0.5 / UNIT,
        ],
        (body.max[0] - body.min[0]) * 0.5 / UNIT,
    )
}

/// Whether a creature no seat moves opens the doors it runs into: only
/// while it walks a route (KOTOR's open the doors their path goes
/// through; one fighting in place pushes no door open), and only alive.
/// A Sith on the Endar Spire, killed as his walk met end_door10_cut2, his
/// walk not yet cleared, opened it, and its scene, kept for a party
/// member's opening (`k_pend_room5_02`: IsObjectPartyMember of
/// GetLastOpenedBy), never played.
pub(crate) fn opens_doors(fighter: &crate::combat::Fighter) -> bool {
    fighter.mind.walker.is_some() && fighter.alive()
}

/// `bodies` less the living ones `mine` (a body's box) already stands inside:
/// a move is not held by a body it overlaps, so it walks out of it (JA's
/// traces hold a box inside another wherever a step leaves it inside, and
/// KOTOR moves no body out of another), and `Inside::hold_off` keeps it
/// from walking further in. Whichever of two overlapping bodies moves, it
/// meets the other alike. Bodies overlap as they meet, round across, as
/// KOTOR's creatures keep their personal space round them.
pub(crate) fn clear_of<'b>(mine: &Body, bodies: &'b [Body]) -> Inside<'b> {
    let (centre, reach) = round_of(mine);
    let overlaps = |other: &Body| {
        let (theirs, radius) = round_of(other);
        other.contents == Contents::BODY
            && other.entity != mine.entity
            && other.min[2] < mine.max[2]
            && mine.min[2] < other.max[2]
            && math::flat_length([theirs[0] - centre[0], theirs[1] - centre[1], 0.0])
                < reach + radius
    };
    if !bodies.iter().any(overlaps) {
        return Inside {
            bodies: std::borrow::Cow::Borrowed(bodies),
            centres: Vec::new(),
        };
    }
    Inside {
        bodies: bodies
            .iter()
            .filter(|other| !overlaps(other))
            .copied()
            .collect(),
        centres: bodies
            .iter()
            .filter(|other| overlaps(other))
            .map(|other| round_of(other).0)
            .collect(),
    }
}

/// How far round a body (metres, across) another counts as touching it:
/// twice the gap a trace stops short of a plane, as a move stopped by it
/// leaves them.
const TOUCH: f32 = 2.0 * collision::CONTACT_GAP;

/// The box `clear_of` weighs a body by: a seat's player's grown across by
/// `TOUCH`, so a body it touches counts as one it stands inside; a
/// creature's as it is.
pub(crate) fn met_as(mine: &Body, seated: bool) -> Body {
    let mut grown = *mine;
    if seated {
        for axis in 0..2 {
            grown.min[axis] -= TOUCH;
            grown.max[axis] += TOUCH;
        }
    }
    grown
}

impl Inside<'_> {
    /// A move from `before` that took `ps` deeper into a body it stood
    /// inside (nearer its centre) keeps only its part round that body (the
    /// part toward its centre taken off), when `clear` lets a box go there
    /// from `before` (a seat's player's, `movement`); else it is taken back
    /// across the floor. It walks out of the other or round it, never into
    /// it, as a trace stopped by the round slides round it. Weighed as
    /// boxes, a step off a box's corner was deeper across one face, and a
    /// body between two others' corners had no way out but back (the Exile
    /// held by the royal soldiers out of Onderon's palace gas room); taken
    /// back whole, a player pressed between a wall and a body it overlapped
    /// (Freyyr, come up to talk in Chuundar's throne room) never got out:
    /// every way its walk tried led nearer him.
    pub(crate) fn hold_off(
        &self,
        before: [f32; 3],
        ps: &mut MotionState,
        clear: impl Fn([f32; 3], [f32; 3]) -> bool,
    ) {
        // Nearer the centre of one it stood inside than it stood, by the
        // step `moved` (weighed from the centre, where a step round it is
        // not lost to the coordinates' rounding far from the origin).
        let from: Vec<[f32; 2]> = self
            .centres
            .iter()
            .map(|centre| [before[0] - centre[0], before[1] - centre[1]])
            .collect();
        let deeper = |moved: [f32; 2]| {
            from.iter().any(|rel| {
                let at = [rel[0] + moved[0], rel[1] + moved[1]];
                at[0] * at[0] + at[1] * at[1] < rel[0] * rel[0] + rel[1] * rel[1]
            })
        };
        let mut moved = [ps.origin[0] - before[0], ps.origin[1] - before[1]];
        if !deeper(moved) {
            return;
        }
        let mut velocity = [ps.velocity[0], ps.velocity[1]];
        for rel in &from {
            let length = math::flat_length([rel[0], rel[1], 0.0]);
            if length <= f32::EPSILON {
                continue;
            }
            let out = [rel[0] / length, rel[1] / length];
            for v in [&mut moved, &mut velocity] {
                let toward = v[0] * out[0] + v[1] * out[1];
                if toward < 0.0 {
                    v[0] -= toward * out[0];
                    v[1] -= toward * out[1];
                }
            }
        }
        let along = [before[0] + moved[0], before[1] + moved[1], ps.origin[2]];
        if !deeper(moved) && clear(before, along) {
            ps.origin = along;
            ps.velocity[0] = velocity[0];
            ps.velocity[1] = velocity[1];
        } else {
            ps.origin[0] = before[0];
            ps.origin[1] = before[1];
            ps.velocity[0] = 0.0;
            ps.velocity[1] = 0.0;
        }
    }
}

/// Whether a standing box (its top `maxs_z` units over its origin) goes
/// from `from` to `to` (origins, units) through `world` unstopped.
pub(crate) fn standing_clear(
    world: &dyn CollisionWorld,
    from: [f32; 3],
    to: [f32; 3],
    maxs_z: f32,
) -> bool {
    let half = motion::HALF_WIDTH * UNIT;
    let extents = Extents {
        mins: [-half, -half, motion::SOLE_Z * UNIT],
        maxs: [half, half, maxs_z * UNIT],
    };
    let trace = world.trace(
        scale(from, UNIT),
        scale(to, UNIT),
        &extents,
        Contents::PLAYER_SOLID,
    );
    !trace.start_solid && trace.fraction >= 1.0
}

/// This step's faces under the placeables play made (`MadeFaces`).
pub(super) fn made_faces(
    level: Res<Static>,
    mut faces: ResMut<MadeFaces>,
    made: Query<(&Object, &Placed), Without<Player>>,
) {
    let placed = level.0.placed().len() as u32;
    let mut objects: Vec<(u32, [f32; 3], Option<Arc<ObjectData>>)> = made
        .iter()
        .filter(|(object, _)| object.kind == ObjectKind::Placeable && object.index >= placed)
        .map(|(object, at)| (object.index, at.0, object.facts.clone()))
        .collect();
    objects.sort_by_key(|(index, ..)| *index);
    let places: Vec<(u32, [f32; 3])> = objects.iter().map(|(index, at, _)| (*index, *at)).collect();
    let facts = |index: u32| {
        objects
            .iter()
            .find(|(of, ..)| *of == index)
            .and_then(|(.., facts)| facts.clone())
    };
    let MadeFaces(now, walls) = &mut *faces;
    let walled = walls.faces(&level.0, &places, &facts);
    if !Arc::ptr_eq(now, walled) {
        *now = walled.clone();
    }
}

/// A character's personal space (metres): its appearance's `perspace`
/// (`module::ObjectData::personal_space`), else a humanoid's.
pub(super) fn personal_space(object: Option<&Object>) -> f32 {
    strikes::humanoid_space(
        object
            .and_then(|object| object.facts.as_ref())
            .map_or(0.0, |facts| facts.personal_space),
    )
}
