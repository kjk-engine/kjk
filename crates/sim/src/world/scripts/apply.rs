//! The scripts' changes put into the world.

use super::*;

/// Puts the scripts' changes into the world (jumps, facings, doors,
/// animations, sounds, sheets), and what they tell players into the
/// step's notices, by seat.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(in crate::world) fn apply(
    mut commands: Commands,
    entities: Query<(Entity, &Id)>,
    level: Res<Static>,
    clock: Res<Clock>,
    mut scripts: ResMut<Scripts>,
    (mut notices, avatars, mut area, mut lost): (
        ResMut<crate::notice::Notices>,
        Query<(&Id, &crate::world::Avatar)>,
        ResMut<crate::world::Module>,
        ResMut<crate::world::falls::Lost>,
    ),
    mut players: Query<(&Id, &mut Player)>,
    mut objects: Query<(&Id, &mut Object)>,
    mut fighters: Query<(&Id, &mut Fighting)>,
    (lives, mut placed): (
        Query<&crate::world::Lifetime>,
        Query<(&Id, &mut Placed), Without<Player>>,
    ),
) {
    // The doors as they stand, for a jump's place.
    let states = level.0.states(
        objects
            .iter()
            .map(|(_, object)| (object.index, &object.state)),
    );
    for (to, notice) in std::mem::take(&mut scripts.notices) {
        let mut seats: Vec<_> = avatars
            .iter()
            .filter(|(id, _)| to.is_none_or(|to| id.0.0 == to.0))
            .map(|(_, avatar)| avatar.0)
            .collect();
        seats.sort();
        notices
            .0
            .extend(seats.into_iter().map(|seat| (seat, notice.clone())));
    }
    if std::mem::take(&mut scripts.lost) && lost.0.is_none() {
        lost.0 = Some(clock.time_ms);
    }
    if let Some(sound) = scripts.area_sound.take() {
        area.sound = sound;
    }
    if let Some(fade) = scripts.fade.take() {
        area.fade = Some(fade);
    }
    if let Some(world_fade_in) = scripts.world_fade_in.take() {
        area.world_fade_in = world_fade_in;
    }
    if let Some(video_effect) = scripts.video_effect.take() {
        area.video_effect = video_effect;
    }
    // The arrival's scripts have run: the engine lifts the fade, unless
    // they held it for a conversation.
    if area.world_fade_in == crate::snapshot::WorldFadeIn::AfterArrival {
        area.world_fade_in = crate::snapshot::WorldFadeIn::Done;
        area.fade = area.fade.map(|fade| fade.lifted(clock.time_ms));
    }
    // A module a script started: the party leaves as a door's link would
    // take it, the first seat asking for everyone.
    if let Some(new) = scripts.new_module.take()
        && let Some(seat) = avatars.iter().map(|(_, avatar)| avatar.0).min()
    {
        area.pending = Some(crate::snapshot::Transition {
            seat,
            via: SimId(new.by.0),
            module: new.module,
            tag: new.tag,
        });
    }
    // Objects scripts made: characters standing where they were put,
    // facing their way, their template's sheet riding in the snapshot.
    // One joining the party as it is made comes in following.
    let joining: Vec<ObjectId> = scripts
        .changes
        .iter()
        .filter_map(|change| match change {
            Change::Join { who } => Some(*who),
            _ => None,
        })
        .collect();
    // The living bodies standing: box origins (units) and personal spaces
    // (metres), for the places of the creatures made.
    let mut stood: Vec<([f32; 3], f32)> = players
        .iter()
        .filter(|(id, _)| {
            fighters
                .iter()
                .any(|(of, fighting)| of.0 == id.0 && fighting.0.alive())
        })
        .map(|(id, player)| {
            let own = objects
                .iter()
                .find(|(of, _)| of.0 == id.0)
                .and_then(|(_, object)| object.facts.as_ref())
                .map_or(0.0, |facts| facts.personal_space);
            (player.0.origin, crate::world::strikes::humanoid_space(own))
        })
        .collect();
    for made in std::mem::take(&mut scripts.made) {
        // One a script destroyed at once in the step that made it is never
        // put in (the entity would come after this step's changes, and the
        // destroy find nothing): TSL's Bao-Dur spawned twice as he joins
        // (`SpawnAvailableNPC`).
        let undone = scripts
            .changes
            .iter()
            .any(|change| matches!(change, Change::Destroy { who, ms: 0, .. } if *who == made.id));
        if undone {
            continue;
        }
        let kind = made.facts.kind;
        let index = area.instances.make(
            &level.0,
            crate::snapshot::Spawned {
                kind,
                template: made.facts.template.clone(),
                yaw: made.yaw,
                holds: crate::world::remains::holds(&made.facts),
            },
        );
        // A placeable or a laid mine: where it was made, no body (nothing
        // collides with it: the area's collision is built once). Its trap
        // bits set this step are its from the start (the entity is put in
        // after this step's changes).
        if kind != ObjectKind::Creature {
            let trap = scripts
                .changes
                .iter()
                .find_map(|change| match change {
                    Change::Trap { object, bits } if *object == made.id => Some(*bits),
                    _ => None,
                })
                .unwrap_or(0);
            commands.spawn((
                Id(crate::SimId(made.id.0)),
                Object {
                    kind,
                    index,
                    state: crate::snapshot::ObjectState {
                        trap,
                        ..crate::snapshot::ObjectState::default()
                    },
                    facts: Some(made.facts),
                },
                Placed(made.at),
            ));
            continue;
        }
        // A companion the party table kept stands up as it was; a sheet
        // a script set on it as it was made (TSL's companion alignment)
        // is its from the start, as its trap bits are.
        let set = scripts
            .changes
            .iter()
            .rev()
            .find_map(|change| match change {
                Change::Sheet { who, sheet } if *who == made.id => Some(*sheet),
                _ => None,
            });
        let sheet = match made.kept {
            Some(kept) => kept.sheet,
            None => made
                .facts
                .sheet
                .unwrap_or_else(crate::combat::Sheet::commoner),
        };
        let sheet = set.unwrap_or(sheet);
        // Where the safe-location search stands it, as a jump's
        // (`install::jump_place`), clear of the living bodies and of those
        // made before it; none found, where it was put.
        let space = crate::world::strikes::humanoid_space(made.facts.personal_space);
        let free = level.0.with(states.as_deref(), |world| {
            crate::install::stand_among(
                &level.0,
                world,
                made.at,
                &stood,
                (space, crate::install::JUMP),
            )
        });
        let origin = free.unwrap_or_else(|| {
            let mut origin = scale(made.at, 1.0 / collision::UNIT);
            origin[2] += 0.125 - crate::motion::SOLE_Z;
            origin
        });
        stood.push((origin, space));
        let mut fighter = crate::combat::Fighter::of(&sheet).hurt_as(made.facts.hit_points);
        if let Some(kept) = made.kept {
            let whole = script::HUNDREDTHS;
            let (most_hp, most_fp) = (
                i32::from(sheet.max_hp) * whole,
                i32::from(sheet.pool()) * whole,
            );
            fighter.hp = kept.hp.clamp(whole, most_hp.max(whole));
            fighter.fp = kept.fp.clamp(0, most_fp);
        }
        if joining.contains(&made.id) {
            join(&mut fighter, true);
        }
        // An animation a script played on it as it was made (TSL's medbay
        // body, made lying dead) is its from the start, as a trap's bits
        // are: the entity is put in after this step's changes.
        fighter.acting = scripts
            .changes
            .iter()
            .rev()
            .find_map(|change| match change {
                Change::Animate {
                    who,
                    play: (animation, ms),
                } if *who == made.id => Some(Acting {
                    animation: *animation,
                    since: clock.time_ms,
                    ms: *ms,
                }),
                _ => None,
            })
            .or(fighter.acting);
        commands.spawn((
            Id(crate::SimId(made.id.0)),
            Object {
                kind,
                index,
                state: crate::snapshot::ObjectState::default(),
                facts: Some(made.facts),
            },
            Placed(made.at),
            Player(crate::motion::MotionState::standing(origin, made.yaw)),
            Fighting(fighter),
            PlayerSheet(sheet),
        ));
    }
    // The step's destroys, by id: the soonest each (the first of equals).
    let mut destroys: BTreeMap<u32, (Entity, u32, Option<Going>)> = BTreeMap::new();
    let changes = std::mem::take(&mut scripts.changes);
    for change in changes {
        match change {
            Change::Sound {
                object,
                state,
                volume,
                pitch,
                play,
                fade,
            } => {
                if let Some((_, mut found)) = objects.iter_mut().find(|(id, _)| id.0.0 == object.0)
                {
                    if let Some(state) = state {
                        found.state.state = state;
                    }
                    if let Some(volume) = volume {
                        found.state.volume = volume;
                    }
                    if let Some(pitch) = pitch {
                        found.state.pitch = pitch;
                    }
                    if play {
                        found.state.plays = found.state.plays.wrapping_add(1);
                    }
                    if let Some(fade) = fade {
                        found.state.fade = fade;
                    }
                }
            }
            Change::Jump { who, to, yaw } => {
                // KOTOR's jump, held world or not (`install::jump_place`):
                // the mark on its walkmesh when it is free, else the first
                // free place of the rings about it within `install::JUMP` (the party
                // jumped beside Kashyyyk's Star Map guardian would stand in
                // its pedestal), clear of the other living bodies by the two
                // bodies' personal spaces; no place, no jump.
                let space = |id: u32| {
                    let own = objects
                        .iter()
                        .find(|(of, _)| of.0.0 == id)
                        .and_then(|(_, object)| object.facts.as_ref())
                        .map_or(0.0, |facts| facts.personal_space);
                    crate::world::strikes::humanoid_space(own) / collision::UNIT
                };
                let alive = |id: SimId| {
                    fighters
                        .iter()
                        .any(|(of, fighting)| of.0 == id && fighting.0.alive())
                };
                let mine = space(who.0);
                let taken: Vec<([f32; 3], f32)> = players
                    .iter()
                    .filter(|(id, _)| id.0.0 != who.0 && alive(id.0))
                    .map(|(id, player)| (player.0.origin, mine + space(id.0.0)))
                    .collect();
                let free = level.0.with(states.as_deref(), |world| {
                    crate::install::jump_place(
                        &level.0,
                        world,
                        to,
                        &taken,
                        (mine * collision::UNIT, crate::install::JUMP),
                    )
                });
                #[cfg(feature = "probes")]
                scripts.report.jumps.push(jumped(
                    (clock.time_ms, who),
                    (
                        to,
                        players
                            .iter()
                            .find(|(id, _)| id.0.0 == who.0)
                            .map_or([0.0; 3], |(_, player)| player.0.origin),
                        crate::install::on_walkmesh(&level.0, to),
                    ),
                    free,
                    players
                        .iter()
                        .filter(|(id, _)| id.0.0 != who.0 && alive(id.0))
                        .map(|(id, player)| (id.0.0, player.0.origin, mine + space(id.0.0))),
                ));
                if let Some(free) = free
                    && let Some((_, mut player)) =
                        players.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    let ps = &mut player.0;
                    ps.origin = free;
                    ps.velocity = [0.0; 3];
                    ps.face(yaw);
                }
            }
            Change::Face { who, yaw } => {
                if let Some((_, mut player)) = players.iter_mut().find(|(id, _)| id.0.0 == who.0) {
                    player.0.face(yaw);
                }
            }
            Change::TakeView { who, angles, view } => {
                if let Some((_, mut player)) = players.iter_mut().find(|(id, _)| id.0.0 == who.0) {
                    player.0.command_angles = angles;
                    player.0.set_view(view);
                }
            }
            Change::Animate {
                who,
                play: (animation, ms),
            } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    // The dead play their death alone.
                    if fighting.0.dead() {
                        continue;
                    }
                    fighting.0.acting = Some(Acting {
                        animation,
                        since: clock.time_ms,
                        ms,
                    });
                } else if let Some(look) = crate::snapshot::look::of_animation(animation)
                    && let Some((_, mut object)) = objects.iter_mut().find(|(id, object)| {
                        id.0.0 == who.0 && object.kind == ObjectKind::Placeable
                    })
                {
                    // A placeable takes the look the animation leaves it in
                    // (`snapshot::look`): the views play its way there.
                    object.state.state = look;
                }
            }
            Change::Struck { who, row } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    crate::script::effects::blows::struck(&mut fighting.0, row, clock.time_ms);
                }
            }
            Change::Walk { who, anew } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    let fighter = &mut fighting.0;
                    // A walk is something else playing: an animation looping
                    // on until then (`u32::MAX`) ends (S7).
                    if fighter.acting.is_some_and(|acting| acting.ms == u32::MAX) {
                        fighter.acting = None;
                    }
                    // A new move after a walk given up walks anew (its
                    // stalls too); one sent again while its walk goes on
                    // keeps its stalls, so a walk re-sent each step (a
                    // fight's chase) still stalls, steps aside and plans
                    // round what holds it.
                    if let Some(walker) = fighter.mind.walker.as_mut() {
                        match anew && walker.stuck() {
                            true => walker.begin_anew(),
                            false => walker.count_bumps_afresh(),
                        }
                    }
                }
            }
            Change::Clear { who } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    let fighter = &mut fighting.0;
                    // A body dying in place keeps its dying clip.
                    let dying = |acting: Acting| acting.animation == crate::combat::death::DIE;
                    if !fighter.acting.is_some_and(dying) {
                        fighter.acting = None;
                    }
                    fighter.mind.last_seen = None;
                    fighter.mind.walker = None;
                    // An item's use its action began goes with the queue.
                    if fighter.pack.ordered {
                        crate::combat::items::break_off(fighter, clock.time_ms);
                    }
                }
            }
            // A door opened by another opens away from it and runs its
            // `OnOpen` with that one its opener: from the next step's
            // scripts, the door open by then. One a script opens itself
            // takes its first way, and runs its `OnOpen` too, its opener
            // itself: Citadel Station's Exchange door is opened by its own
            // `ActionOpenDoor` (`k_drd_death`) and its `OnOpen` (`a_luxa`)
            // begins Slusk's scene.
            Change::Door { door, open, by } => {
                let Some((_, mut object)) = objects.iter_mut().find(|(id, _)| id.0.0 == door.0)
                else {
                    continue;
                };
                if (object.state.state != 0) == open {
                    continue;
                }
                // A door does not close on someone standing in its doorway
                // (KOTOR's stays open): it would shut them in its panels
                // (Manaan's Sith base, its wing door behind an apprentice).
                let index = object.index;
                let standing_in = players.iter().any(|(id, player)| {
                    let alive = fighters
                        .iter()
                        .find(|(of, _)| of.0 == id.0)
                        .is_none_or(|(_, fighting)| fighting.0.alive());
                    alive
                        && crate::world::in_doorway(
                            &level.0,
                            index,
                            player.0.origin,
                            player.0.maxs_z,
                        )
                });
                // Nor does one broken open (`world::objects`).
                let broken =
                    (object.facts.as_deref()).is_some_and(|facts| object.state.broken(facts));
                if !open && (standing_in || broken) {
                    continue;
                }
                let from = players
                    .iter()
                    .find(|(id, _)| id.0.0 == by.0 && by != door)
                    .map(|(_, player)| scale(player.0.origin, collision::UNIT));
                let facts = area.instances.get(object.index);
                object.state.state = match facts {
                    Some(facts) if open => facts.opening(from),
                    _ => u8::from(open),
                };
                if open {
                    let opened = (door.0, script::last::OPENED);
                    scripts.state.last.insert(opened, by.0);
                    if let Some(script) = facts.and_then(|facts| Hook::Open.of(facts)) {
                        scripts.state.pending.push(Pending::new(script, door));
                    }
                }
            }
            Change::Lock { door, locked } => {
                if let Some((_, mut object)) = objects.iter_mut().find(|(id, _)| id.0.0 == door.0) {
                    object.state.lock = if locked { lock::LOCKED } else { lock::UNLOCKED };
                }
            }
            Change::Reveal(how) => {
                let chart = level.0.module().and_then(|module| module.map);
                let module = area.place.as_ref().map(|place| place.module.clone());
                if let (Some(chart), Some(module)) = (chart, module) {
                    crate::fog::reveal(&mut area.explored, &module, &chart, how);
                }
            }
            Change::MapPin {
                pin: waypoint,
                enabled,
            } => {
                if let Some((_, mut object)) = objects.iter_mut().find(|(id, object)| {
                    id.0.0 == waypoint.0 && object.kind == ObjectKind::Waypoint
                }) {
                    use crate::snapshot::pin;
                    object.state.state = if enabled { pin::ON } else { pin::OFF };
                }
            }
            Change::Trap { object, bits } => {
                if let Some((_, mut found)) = objects.iter_mut().find(|(id, _)| id.0.0 == object.0)
                {
                    found.state.trap = bits;
                }
            }
            // A companion follows the first seat and fights in the
            // player's faction; one that leaves is on its own again.
            Change::Join { who } | Change::Leave { who } => {
                let joins = matches!(change, Change::Join { .. });
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    join(&mut fighting.0, joins);
                }
            }
            Change::Faction { who, faction } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    fighting.0.faction = Some(faction);
                }
            }
            Change::Hand { who, ms } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    let until = clock.time_ms + u64::from(ms);
                    fighting.0.force.hand = Some((crate::combat::force::Gesture::Push, until));
                }
            }
            Change::Pay { who, cost } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    crate::combat::effects::charge(&mut fighting.0, cost, clock.time_ms);
                }
            }
            Change::Stealth { who, on } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    fighting.0.covert.stealth = on;
                }
            }
            // A conversation began: those it brings, far off, come to it
            // (R1): a player's own its seat's companions no seat drives, a
            // party one every member and the seats' bodies (I30 P7).
            Change::Gather { around, who } => {
                let member = |id: &Id, fighting: &Fighting| {
                    let mind = &fighting.0.mind;
                    fighting.0.alive()
                        && match who {
                            script::Gathered::Companions(seat) => {
                                mind.leader == Some(crate::input::Seat(seat))
                                    && mind.driver.is_none()
                                    || avatars.iter().any(|(of, avatar)| {
                                        of.0 == id.0 && avatar.0 == crate::input::Seat(seat)
                                    })
                            }
                            script::Gathered::Everyone => {
                                let seat = avatars.iter().any(|(of, _)| of.0 == id.0);
                                // A seat's body standing in a player's own
                                // conversation stays in it (R5).
                                let elsewhere = (seat || mind.driver.is_some())
                                    && scripts
                                        .state
                                        .talk
                                        .conversations
                                        .iter()
                                        .any(|c| !c.party && c.holds(ObjectId(id.0.0)));
                                (mind.leader.is_some() || seat) && !elsewhere
                            }
                        }
                };
                let mut members: Vec<SimId> = fighters
                    .iter()
                    .filter(|(id, fighting)| member(id, fighting))
                    .map(|(id, _)| id.0)
                    .collect();
                members.sort();
                let states = level.0.states(
                    objects
                        .iter()
                        .map(|(_, object)| (object.index, &object.state)),
                );
                crate::world::coop_talk::gather(
                    &level.0,
                    states.as_deref(),
                    SimId(around.0),
                    &members,
                    &mut players,
                );
            }
            Change::HitPoints { who, hp } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                    && fighting.0.alive()
                {
                    fighting.0.hp = i32::from(hp.max(1)) * script::HUNDREDTHS;
                }
            }
            Change::Levelled { who, before, after } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                    && fighting.0.alive()
                {
                    let fighter = &mut fighting.0;
                    let now =
                        |sheet| crate::combat::effects::sheet_now(sheet, fighter, clock.time_ms);
                    let (before, after) = (now(before), now(after));
                    let gained = i32::from(after.force_points()) - i32::from(before.force_points());
                    fighter.hp = i32::from(after.max_hp) * script::HUNDREDTHS;
                    fighter.fp = (fighter.fp + gained * script::HUNDREDTHS).max(0);
                }
            }
            Change::CancelCombat { who } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    let fighter = &mut fighting.0;
                    fighter.target = None;
                    fighter.combat_until = fighter.combat_until.min(clock.time_ms);
                    fighter.saber.out_until = fighter.saber.out_until.min(clock.time_ms);
                }
            }
            Change::InCombat { who } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    let fighter = &mut fighting.0;
                    fighter.combat_until = fighter.combat_until.max(clock.time_ms + COMBAT_MS);
                }
            }
            // TSL's fake combat: taken only out of a real fight, held until
            // a script lets go; let go only while it fakes.
            Change::FakeCombat { who, on } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    let fighter = &mut fighting.0;
                    let real = fighter.combat_until > clock.time_ms;
                    if on && !real {
                        fighter.fake_until = crate::combat::fighter::HELD;
                    } else if !on && fighter.faking(clock.time_ms) {
                        fighter.fake_until = 0;
                    }
                }
            }
            // The weapon flourished (TSL's `CreatureFlourishWeapon`), once a
            // second at most, not by one fallen: out (a holstered one too),
            // in fake combat for `COMBAT_MS` unless it fights for real, so a
            // lightsaber lights with its style's draw flourish (`move_body`'s
            // `draws`). [verify] the executable's own flourish clip (10158)
            // on a blade already lit.
            Change::Flourish { who } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    let fighter = &mut fighting.0;
                    let now = clock.time_ms;
                    if fighter.alive() && fighter.flourish_ready <= now {
                        fighter.flourish_ready = now + FLOURISH_MS;
                        if fighter.sheath != Sheath::Lit {
                            fighter.sheath = Sheath::Fight;
                        }
                        if fighter.combat_until <= now {
                            fighter.fake_until = fighter.fake_until.max(now + COMBAT_MS);
                        }
                    }
                }
            }
            Change::SaberPower { who, on, held } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                    && fighting.0.alive()
                {
                    fighting.0.saber.on = on;
                    fighting.0.sheath = if held { Sheath::Lit } else { Sheath::Fight };
                }
            }
            Change::Destroy { who, ms, going } => {
                if let Some((entity, _)) = entities.iter().find(|(_, id)| id.0.0 == who.0)
                    && destroys
                        .get(&who.0)
                        .is_none_or(|(_, soonest, _)| ms < *soonest)
                {
                    destroys.insert(who.0, (entity, ms, going));
                }
            }
            // The sheet rides in the snapshot from now on (a creature's
            // module sheet no longer says what it wears).
            Change::Sheet { who, sheet } => {
                if let Some((entity, _)) = entities.iter().find(|(_, id)| id.0.0 == who.0) {
                    commands.entity(entity).insert(PlayerSheet(sheet));
                }
            }
            Change::UseItem { who, row } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    fighting.0.pack.queued = Some(row);
                }
            }
            Change::Swapped { who } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                    && fighting.0.combat_until > clock.time_ms
                {
                    let fighter = &mut fighting.0;
                    fighter.weapon_wait_ms = fighter.weapon_wait_ms.max(0) + crate::combat::SWAP_MS;
                }
            }
            Change::Usable { who, sheet, used } => {
                if let Some((entity, _)) = entities.iter().find(|(_, id)| id.0.0 == who.0) {
                    commands.entity(entity).insert(PlayerSheet(sheet));
                }
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    fighting.0.pack.used = used;
                }
            }
            Change::Look { who, look } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    fighting.0.look = look;
                }
            }
            Change::Place { object, to } => {
                if let Some((_, mut at)) = placed.iter_mut().find(|(id, _)| id.0.0 == object.0) {
                    at.0 = to;
                }
            }
            // Out of stealth without a word (`SetActivity`'s, as a
            // conversation begins).
            Change::Unstealth { who } => {
                if let Some((_, mut fighting)) = fighters.iter_mut().find(|(id, _)| id.0.0 == who.0)
                {
                    fighting.0.covert.stealth = false;
                }
            }
        }
    }
    // Out of the world after `ms`, drawn going; one going sooner already
    // keeps its way (the first destroy event taken), as does a body
    // already fading out (its fade goes on to its end).
    for (entity, ms, going) in destroys.into_values() {
        if lives
            .get(entity)
            .is_ok_and(|life| life.left_ms <= ms || matches!(life.going, Some(Going::Fades(_))))
        {
            continue;
        }
        commands.entity(entity).insert(crate::world::Lifetime {
            left_ms: ms.max(1),
            going,
        });
    }
}

/// A companion follows the first seat in the player's faction; one that
/// leaves is on its own again.
pub(in crate::world) fn join(fighter: &mut crate::combat::Fighter, joins: bool) {
    let party = crate::combat::sheet::factions::PLAYER;
    fighter.mind.leader = joins.then_some(crate::world::LEADER_SEAT);
    // One leaving keeps a faction a script gave it (Mission and Zaalbar
    // turned hostile, then taken from the party, on the Unknown World's
    // dark way); only the party's own goes.
    if joins {
        fighter.faction = Some(party);
    } else if fighter.faction == Some(party) {
        fighter.faction = None;
    }
}
