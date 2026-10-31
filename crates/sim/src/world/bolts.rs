//! Bolts in flight, grenades lobbed and their blasts.

use super::*;

/// Bolts in id order: each traced from where it was a step ago to where it
/// is, against the level, the living bodies its owner's shots meet
/// (`Targets`) but the owner's own, and the tick's saber boxes
/// (`sabers::Blades`); a body it meets is hit, a wall stops it, and it goes
/// unseen when its life is out. A bolt that starts inside something is
/// traced again where it stands, to learn what it is in (JA's
/// `G_RunMissile`): a body it is in is hit. On the authority a lightsaber
/// may turn it first (`sabers::bolt_block`: absorbed, or sent on as its
/// blocker's), on its holder's body by state or on the blade itself. A bolt
/// a remote player could block meets its body and is held there for its
/// grace (`missiles::Held`, C19): turned by a late block, or landing when
/// the hold ends.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn fly(
    clock: Res<Clock>,
    level: Res<Static>,
    inputs: Res<Inputs>,
    blades: Res<sabers::Blades>,
    mut streams: ResMut<Streams>,
    (mut hits, mut battering): (ResMut<Hits>, ResMut<objects::Battering>),
    mut scripts: ResMut<scripts::Scripts>,
    mut casts: ResMut<scripts::Casts>,
    objects: Query<&Object>,
    placed: Query<(&Id, &Object)>,
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &Player,
        &mut Fighting,
    )>,
    mut bolts: Query<(Entity, &Id, &mut Bolt)>,
    mut commands: Commands,
) {
    let now = clock.time_ms;
    let authority = clock.reason != StepReason::Predict;
    let states = door_states(&level.0, &objects);
    let shots = sides_of(
        &level.0,
        &scripts,
        actors
            .iter()
            .map(|(id, avatar, object, sheet, _, fighting)| {
                (id.0, avatar, object, sheet, fighting)
            }),
    );
    let targets = Targets::of(
        level.0.friendly_fire,
        actors
            .iter()
            .map(|(id, avatar, object, _, player, fighting)| {
                (
                    id.0,
                    &player.0,
                    object_bulk(object),
                    fighting.0.alive(),
                    in_party(avatar, &fighting.0),
                )
            }),
    );
    let mut actors: Vec<_> = actors.iter_mut().collect();
    actors.sort_by_key(|(id, ..)| id.0);
    let origins: Vec<(SimId, [f32; 3])> = actors
        .iter()
        .map(|(id, .., player, _)| (id.0, player.0.origin))
        .collect();
    let find = |id: SimId| origins.binary_search_by_key(&id, |(own, _)| *own).ok();
    // The living's chests, what a blast reaches.
    let chests: Vec<(SimId, [f32; 3])> = actors
        .iter()
        .filter(|(.., fighting)| fighting.0.alive())
        .map(|(id, _, object, _, player, _)| {
            (id.0, mind::chest_of(&player.0, object_bulk(*object)))
        })
        .collect();
    // The placeables that may be bashed, as a player's own bolt meets them:
    // their bodies, as a fist does (a footlocker has no brushes to stop a
    // shot: TSL's prologue's, K1's). A door's brushes stop a shot already;
    // a companion's bolts fly at its foes past them, as before.
    let bashable = objects::bodies(
        placed
            .iter()
            .filter(|(_, object)| object.kind == ObjectKind::Placeable),
    );
    let mut bolts: Vec<_> = bolts.iter_mut().collect();
    bolts.sort_by_key(|(_, id, _)| id.0);
    for (entity, _, mut bolt) in bolts {
        let bolt = &mut bolt.0;
        if bolt.lob.is_some() {
            let burst = lob(
                &level.0,
                states.as_deref(),
                bolt,
                &shots.met(bolt.owner, targets.met_by(bolt.owner)),
                now,
                clock.msec,
            );
            if let Some(at) = burst {
                // A grenade whose row has an impact script (`k_sup_grenade`)
                // is cast where it bursts, its script deciding whom it
                // catches and what it does to them; else the native blast.
                let row = bolt.lob.map(|lob| lob.row);
                let scripted = row.filter(|row| {
                    level
                        .0
                        .scripts()
                        .is_some_and(|library| library.impact(*row).is_some())
                });
                // What may be bashed is caught by any blast (`objects`), its
                // body's middle: KOTOR's grenade script asks for creatures
                // alone.
                let bashed: Vec<(SimId, [f32; 3])> = objects::bodies(placed.iter())
                    .iter()
                    .map(|body| (SimId(body.entity & !BODY), objects::middle(body)))
                    .collect();
                let states = states.as_deref();
                match scripted {
                    Some(row) if authority => {
                        casts.0.push(crate::script::Cast {
                            caster: script_ncs::vm::ObjectId(bolt.owner.0),
                            row,
                            target: script_ncs::vm::ObjectId::INVALID,
                            at: scale(at, UNIT),
                            caught: caught_by_blast(&level.0, states, &chests, at)
                                .into_iter()
                                .map(|(id, _)| script_ncs::vm::ObjectId(id.0))
                                .collect(),
                        });
                        blast(&level.0, states, &bashed, bolt, at, &mut hits.0);
                    }
                    Some(_) => {}
                    None => {
                        let caught: Vec<(SimId, [f32; 3])> =
                            chests.iter().chain(&bashed).copied().collect();
                        blast(&level.0, states, &caught, bolt, at, &mut hits.0);
                    }
                }
                commands.entity(entity).despawn();
            }
            continue;
        }
        if let Some(hold) = bolt.held {
            // Held for its target's grace: turned by a late block, or lands.
            let Some(at_target) = find(hold.target).filter(|_| authority) else {
                if authority {
                    commands.entity(entity).despawn();
                }
                continue;
            };
            let shooter = find(bolt.owner).map(|by| origins[by].1);
            let (_, avatar, object, sheet, player, fighting) = &mut actors[at_target];
            let sheet = sheet_of(*sheet, *object);
            let at = bolt.base;
            let saw = avatar.is_some_and(|avatar| {
                inputs.0.get(avatar.0).is_some()
                    && grace::saw_before(
                        clock.tick,
                        inputs.0.view_lag(avatar.0),
                        clock.msec,
                        hold.tick,
                    )
            });
            let fate = saw
                .then(|| {
                    sabers::bolt_block(
                        &mut fighting.0,
                        &player.0,
                        &sheet,
                        bolt,
                        at,
                        false,
                        shooter,
                        (now, clock.tick),
                        &mut streams.authority,
                    )
                })
                .flatten();
            if let Some(fate) = fate {
                bolt.held = None;
                if turn(bolt, fate, at, hold.target, now) {
                    commands.entity(entity).despawn();
                }
            } else if clock.tick >= hold.until() {
                hits.0.push(bolt_hit(bolt, hold.target, at));
                commands.entity(entity).despawn();
            }
            continue;
        }
        if now >= bolt.until_ms {
            commands.entity(entity).despawn();
            continue;
        }
        let from = bolt.at(now - u64::from(clock.msec));
        let to = bolt.at(now);
        let half = bolt.half_size * UNIT;
        let extents = Extents {
            mins: [-half; 3],
            maxs: [half; 3],
        };
        // The bodies it may hit and the saber boxes that may turn it
        // (rulebook 5.6), but its owner's.
        let mut bodies = shots.met(bolt.owner, targets.met_by(bolt.owner));
        bodies.extend(blades.bodies(bolt.owner));
        let player = find(bolt.owner).is_some_and(|at| actors[at].1.is_some());
        if player {
            bodies.extend(bashable.iter().cloned());
        }
        bodies.sort_by_key(|body| body.entity);
        let trace = |to: [f32; 3]| {
            level.0.with(states.as_deref(), |world| {
                Bodies::new(world, &bodies)
                    .skipping(bolt.owner.0 | BODY)
                    .trace(scale(from, UNIT), scale(to, UNIT), &extents, SHOT)
            })
        };
        let mut met = trace(to);
        if !met.hit() {
            continue;
        }
        if met.start_solid && met.entity.is_none() {
            met = trace(from);
        }
        let at = scale(met.end, 1.0 / UNIT);
        // A script's shot is the script's: no blade turns it, and it lands
        // only its blow, on its target.
        if bolt.scripted {
            let body = met
                .entity
                .filter(|entity| entity & (BODY | sabers::SABER) == BODY);
            if let Some(entity_hit) = body.filter(|_| authority)
                && let Some((hit, row)) = scripted_hit(
                    &mut scripts.state.strikes,
                    bolt,
                    SimId(entity_hit & !BODY),
                    at,
                )
            {
                if let Some(at_target) = find(hit.target) {
                    crate::script::effects::blows::struck(&mut actors[at_target].5.0, row, now);
                }
                hits.0.push(hit);
            }
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(entity_hit) = met.entity.filter(|entity| entity & BODY != 0) {
            let on_blade = entity_hit & sabers::SABER != 0;
            let target = SimId(entity_hit & !(BODY | sabers::SABER));
            // A saber may turn it: the server's call (C20).
            if authority && let Some(at_target) = find(target) {
                let shooter = find(bolt.owner).map(|by| origins[by].1);
                let (_, avatar, object, sheet, player, fighting) = &mut actors[at_target];
                let sheet = sheet_of(*sheet, *object);
                let fate = sabers::bolt_block(
                    &mut fighting.0,
                    &player.0,
                    &sheet,
                    bolt,
                    at,
                    on_blade,
                    shooter,
                    (now, clock.tick),
                    &mut streams.authority,
                );
                if let Some(fate) = fate {
                    if turn(bolt, fate, at, target, now) {
                        commands.entity(entity).despawn();
                    }
                    continue;
                }
                // On the body of a remote player who could block it: held
                // there for its grace.
                let wait = avatar.map_or(0, |avatar| {
                    grace::wait(inputs.0.view_lag(avatar.0), clock.msec)
                });
                if !on_blade && wait > 0 && sabers::could_turn(&fighting.0, &sheet, bolt) {
                    bolt.base = at;
                    bolt.from_ms = now;
                    bolt.held = Some(missiles::Held {
                        target,
                        tick: clock.tick,
                        wait,
                    });
                    continue;
                }
            }
            // A blade that turns nothing stops it all the same.
            if !on_blade {
                hits.0.push(bolt_hit(bolt, target, at));
            }
        } else {
            battering.shot(met.entity, bolt.owner, bolt_source(bolt));
        }
        commands.entity(entity).despawn();
    }
}

/// `bolt` turned at `at` by `blocker` at `now` (`sabers::bolt_block`'s
/// fate): sent on as the blocker's; whether it is gone (absorbed).
pub(super) fn turn(
    bolt: &mut Missile,
    fate: BoltFate,
    at: [f32; 3],
    blocker: SimId,
    now: u64,
) -> bool {
    match fate {
        BoltFate::Absorbed => true,
        BoltFate::Turned(direction) => {
            let speed = math::length(bolt.velocity);
            bolt.base = at;
            bolt.from_ms = now;
            bolt.velocity = scale(direction, speed).map(f32::round_ties_even);
            bolt.owner = blocker;
            bolt.until_ms = now + weapons::BOLT_LIFE_MS;
            false
        }
    }
}

/// `bolt`'s hit on `target` at `at`, along its flight.
pub(super) fn bolt_hit(bolt: &Missile, target: SimId, at: [f32; 3]) -> Hit {
    let mut direction = bolt.velocity;
    math::normalize(&mut direction);
    Hit {
        attacker: bolt.owner,
        target,
        source: bolt_source(bolt),
        point: Some(at),
        direction: Some(direction),
    }
}

/// A script's shot `bolt` meeting `target` at `at`: the blow its script
/// decided its owner lands on it (`CutsceneAttack`'s hit), taken from those
/// waiting, landed now as the script's damage from the shot's weapon, with
/// the row its target answers with; none for anyone else, a miss, or a
/// blow landed already.
pub(super) fn scripted_hit(
    strikes: &mut Vec<crate::script::effects::Strike>,
    bolt: &Missile,
    target: SimId,
    at: [f32; 3],
) -> Option<(Hit, u16)> {
    let index = strikes.iter().position(|strike| {
        strike.by.0 == bolt.owner.0 && strike.target.0 == target.0 && strike.landed
    })?;
    let strike = strikes.remove(index);
    let mut direction = bolt.velocity;
    math::normalize(&mut direction);
    let hit = Hit {
        attacker: bolt.owner,
        target,
        source: Source::Script {
            damage: strike.amount,
            damage_type: None,
            cast: false,
            shot: Some(bolt.weapon.delivery),
            staged: true,
        },
        point: Some(at),
        direction: Some(direction),
    };
    Some((hit, strike.answer))
}

/// What `bolt` strikes with: its weapon's attack.
pub(super) fn bolt_source(bolt: &Missile) -> Source {
    Source::Weapon {
        weapon: bolt.weapon,
        cadence_ms: bolt.cadence_ms,
        share: bolt.share,
        area: false,
    }
}

/// A grenade's step (`combat::grenades`): traced from where it was to
/// where it is against the level and the bodies its owner's shots meet; a
/// body bursts it there, a surface bounces it (or it comes to rest), and
/// its fuse's end bursts it where it is. Where it bursts, if it does.
pub(super) fn lob(
    level: &Level,
    states: Option<&[u8]>,
    bolt: &mut Missile,
    bodies: &[Body],
    now: u64,
    msec: u32,
) -> Option<[f32; 3]> {
    let resting = bolt.lob.is_some_and(|lob| lob.resting);
    if !resting {
        let (from, to) = (bolt.at(now - u64::from(msec)), bolt.at(now));
        let half = bolt.half_size * UNIT;
        let extents = Extents {
            mins: [-half; 3],
            maxs: [half; 3],
        };
        let met = level.with(states, |world| {
            Bodies::new(world, bodies)
                .skipping(bolt.owner.0 | BODY)
                .trace(scale(from, UNIT), scale(to, UNIT), &extents, SHOT)
        });
        let at = scale(met.end, 1.0 / UNIT);
        if met.entity.is_some_and(|entity| entity & BODY != 0) {
            return Some(at);
        }
        if met.hit() {
            // Off the surface a little, so the next trace starts clear.
            let normal = met.plane.normal;
            let bounced = grenades::bounce(bolt.velocity_at(now), normal);
            bolt.base = math::mad(at, 0.25, normal);
            bolt.from_ms = now;
            bolt.velocity = bounced.unwrap_or([0.0; 3]);
            if let (None, Some(lob)) = (bounced, bolt.lob.as_mut()) {
                lob.resting = true;
            }
        }
    }
    (now >= bolt.until_ms).then(|| bolt.at(now))
}

/// A grenade's blast at `at`: a hit on every living character whose chest
/// is within the radius with nothing solid between (the thrower too: the
/// friendly-fire rule decides), pushing away from it.
/// Whom a grenade bursting at `at` (units) reaches: each living chest in
/// `chests` within its radius with nothing solid between, and the way from
/// the burst to it.
pub(super) fn caught_by_blast(
    level: &Level,
    states: Option<&[u8]>,
    chests: &[(SimId, [f32; 3])],
    at: [f32; 3],
) -> Vec<(SimId, [f32; 3])> {
    // From a little above where it lies, so a floor does not hide it.
    let centre = [at[0], at[1], at[2] + 8.0];
    chests
        .iter()
        .filter_map(|&(target, chest)| {
            let mut away = math::sub(chest, centre);
            if math::length(away) > grenades::RADIUS {
                return None;
            }
            let clear = level.with(states, |world| {
                world
                    .trace(
                        scale(centre, UNIT),
                        scale(chest, UNIT),
                        &Extents::POINT,
                        Contents::SOLID,
                    )
                    .fraction
                    >= 1.0
            });
            math::normalize(&mut away);
            clear.then_some((target, away))
        })
        .collect()
}

pub(super) fn blast(
    level: &Level,
    states: Option<&[u8]>,
    chests: &[(SimId, [f32; 3])],
    bolt: &Missile,
    at: [f32; 3],
    hits: &mut Vec<Hit>,
) {
    let Some(lob) = bolt.lob else {
        return;
    };
    for (target, direction) in caught_by_blast(level, states, chests, at) {
        hits.push(Hit {
            attacker: bolt.owner,
            target,
            source: Source::Blast { row: lob.row },
            point: None,
            direction: Some(direction),
        });
    }
}
