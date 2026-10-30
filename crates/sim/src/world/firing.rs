//! Weapons fired in the characters' move order, and the targets and sides
//! a shot meets.

use super::*;

/// Weapons, in the characters' move order: each ranged weapon's cadence
/// (`weapons::cadence`) with its trigger; a shot leaves the muzzle as
/// bolts (entities) or, for a hitscan, strikes at once. A creature's
/// trigger is its mind's, which runs on the authority only, so prediction
/// fires the players' weapons alone. Spread comes from a hash of the tick,
/// the shooter and the shard (`SimRng::new` as the hash), not a stream: a
/// predicted shot spreads as the authority's does, whoever else fired.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn fire(
    clock: Res<Clock>,
    level: Res<Static>,
    blades: Res<sabers::Blades>,
    inputs: Res<Inputs>,
    history: Res<history::History>,
    sent: Res<TorsoSent>,
    mut next_id: ResMut<NextId>,
    (mut hits, mut battering): (ResMut<Hits>, ResMut<objects::Battering>),
    mut heals: ResMut<Heals>,
    scripts: Res<scripts::Scripts>,
    (mut casts, mut spent): (ResMut<scripts::Casts>, ResMut<scripts::Spent>),
    objects: Query<&Object>,
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Player,
        &mut Fighting,
    )>,
    mut commands: Commands,
) {
    let states = door_states(&level.0, &objects);
    spent.0.clear();
    // An item whose row has an impact script is cast, its script doing what
    // it does (`combat::items::press`).
    let scripted = |row: u16| {
        level
            .0
            .scripts()
            .is_some_and(|library| library.impact(row).is_some())
    };
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
    // Each living character's side and place, for a squad stim's reach.
    let sides: Vec<(SimId, [f32; 3], bool, u8)> = actors
        .iter()
        .filter(|(.., fighting)| fighting.0.alive())
        .map(|(id, avatar, object, sheet, player, fighting)| {
            let faction = faction_of(&fighting.0, &sheet_of(sheet, object));
            (
                id.0,
                player.0.origin,
                in_party(avatar, &fighting.0),
                faction,
            )
        })
        .collect();
    let mut actors: Vec<_> = actors.iter_mut().collect();
    actors.sort_by_key(|(id, avatar, ..)| (avatar.is_none(), avatar.map(|a| a.0), id.0));
    for (id, avatar, object, sheet, mut player, mut fighting) in actors {
        if clock.reason == StepReason::Predict && avatar.is_none() {
            continue;
        }
        let fighter = &mut fighting.0;
        // A grip's victim cannot fire (JA's crippled), nor can the stunned,
        // nor a hand held out (JA's hand extends), nor anyone changing
        // weapons or knocked down (the weapon step keeps the timer then).
        let force = &fighter.force;
        if force.gripped_by.is_some()
            || force.stunned(clock.time_ms)
            || force.hand_out(clock.time_ms)
            || fighter.saber.change != Change::None
            || fighter.saber.floored()
        {
            // Its buttons' edges still move on: a press meanwhile is a
            // press after.
            let held = fighter.last_command.buttons;
            fighter.shooting.interrupt();
            fighter.shooting.alt_held = held & buttons::ALT_ATTACK != 0;
            fighter.pack.hold(held);
            fighter.pack.charging_from = None;
            continue;
        }
        // The bar's size is the sheet's own (`Sheet::pool`), whatever runs
        // on it; the rest reads what runs on it.
        let base = sheet_of(sheet, object);
        let sheet = effects::sheet_now(base, fighter, clock.time_ms);
        // An item: used on a press; the weapon waits for it.
        let authority = clock.reason != StepReason::Predict;
        let held_buttons = if fighter.alive() {
            fighter.last_command.buttons
        } else {
            0
        };
        let items::Pressed {
            squad,
            used,
            cast,
            spent: row,
        } = items::press(
            fighter,
            &sheet,
            held_buttons,
            clock.time_ms,
            authority,
            scripted,
        );
        // The item leaves its holder's goods as it is used
        // (`item_uses::spend`).
        if let Some(row) = row.filter(|_| authority) {
            spent.0.push((id.0, row));
        }
        if let Some(row) = cast.filter(|_| authority) {
            casts.0.push(crate::script::Cast {
                caster: script_ncs::vm::ObjectId(id.0.0),
                row,
                target: script_ncs::vm::ObjectId(id.0.0),
                at: player.0.origin.map(|axis| axis * UNIT),
                caught: Vec::new(),
            });
        }
        // A squad stim heals its user's side nearby (the party, or its
        // faction).
        if let Some(points) = squad {
            let mine = sides.iter().find(|(own, ..)| *own == id.0);
            for &(other, at, party, faction) in &sides {
                let near = mine.is_some_and(|&(_, here, my_party, my_faction)| {
                    math::length(math::sub(at, here)) <= items::SQUAD_REACH
                        && party == my_party
                        && (party || faction == my_faction)
                });
                if near {
                    heals.0.push((other, i32::from(points) * 100));
                }
            }
        }
        if used {
            let wait = fighter
                .pack
                .using
                .map_or(items::QUICK_MS, |(_, until)| (until - clock.time_ms) as i32);
            fighter.weapon_wait_ms = fighter.weapon_wait_ms.max(wait);
        }
        if used || fighter.pack.using.is_some() {
            // A gun's timer runs on while a heal is used (a blade's runs in
            // its own step), so it is ready when the heal lands.
            if !used && held(&sheet, fighter).delivery.ranged() {
                weapons::count_down(&mut fighter.weapon_wait_ms, clock.msec);
            }
            continue;
        }
        // A grenade: charged while its button is held, thrown on release,
        // whatever is in hand.
        let throwing = fighter.alive() && fighter.last_command.buttons & buttons::GRENADE != 0;
        if let Some((carried, strength)) = grenades::pull(
            &mut fighter.pack,
            &sheet.items,
            fighter.weapon_wait_ms,
            throwing,
            clock.time_ms,
        ) {
            if authority {
                spent.0.push((id.0, carried.row));
            }
            fighter.weapon_wait_ms = fighter.weapon_wait_ms.max(0) + grenades::REFIRE_MS;
            fighter.combat_until = clock.time_ms + COMBAT_MS;
            fighter.force.tricked = 0;
            motion::react(
                &mut player.0,
                &level.0.anims,
                motion::Anim::BOTH_ATTACK3,
                true,
            );
            let ps = &player.0;
            let (forward, ..) = math::angle_vectors(ps.view);
            let mut velocity = scale(forward, grenades::SPEED * strength);
            velocity[2] += grenades::LIFT;
            commands.spawn((
                Id(SimId(next_id.0)),
                Bolt(Missile {
                    owner: id.0,
                    weapon: sheet.main_weapon(),
                    cadence_ms: 0,
                    share: 1000,
                    base: muzzle(ps, grenades::MUZZLE),
                    from_ms: clock.time_ms,
                    velocity: velocity.map(f32::round_ties_even),
                    half_size: grenades::HALF_SIZE,
                    blockable: false,
                    until_ms: clock.time_ms + grenades::FUSE_MS,
                    lob: Some(grenades::Lob {
                        row: carried.row,
                        resting: false,
                    }),
                    held: None,
                    scripted: false,
                }),
            ));
            next_id.0 += 1;
            continue;
        }
        let weapon = held(&sheet, fighter);
        // The grenade button takes over from the trigger: a gun's timer
        // runs out (a blade's runs in its own step) and the charge begins.
        let readying = throwing
            && fighter
                .pack
                .next(&sheet.items, |u| u == items::Use::Grenade)
                .is_some();
        if readying || fighter.pack.charging_from.is_some() {
            if weapon.delivery.ranged() {
                weapons::count_down(&mut fighter.weapon_wait_ms, clock.msec);
            }
            continue;
        }
        let Delivery::Blaster(blaster) = weapon.delivery else {
            fighter.shooting.interrupt();
            continue;
        };
        // A gun put away fires not: the press raises it first (`move_body`).
        if fighter.weapon_away(clock.time_ms) {
            continue;
        }
        // A pistol in each hand: each gun its own (`combat::pair`).
        let pair = sheet.pistol_pair();
        let blasters = [0, 1].map(|hand| match in_hand(&sheet, fighter, hand).delivery {
            Delivery::Blaster(own) if pair => own,
            _ => blaster,
        });
        let special = specials::Special::of(&sheet, blaster);
        let spec = weapons::primary(blaster);
        // A creature's mind spaces its shots (`combat::troop`): their damage
        // keeps to that pace (C2), and they spread by its aim.
        let creature = avatar.is_none() && fighter.mind.driver.is_none();
        let troop = fighter.mind.troop;
        let paced = (creature && troop.pace_ms > 0).then_some(troop.pace_ms);
        // A scene's attacker (`CutsceneAttack`, a passive attack) fires the
        // script's shots, and a gunner with a blow decided for it (an
        // assured one) that blow's: what they do is the script's (`fly`),
        // fired when its clip fires them (`minds::scene_attack`), whatever
        // the gun's own refire.
        let state = &scripts.state;
        let scripted = state
            .intents
            .get(&id.0.0)
            .is_some_and(|intent| intent.passive)
            || state.strikes.iter().any(|strike| strike.by.0 == id.0.0);
        let pressed = |button: u16| fighter.alive() && fighter.last_command.buttons & button != 0;
        let refires = blasters.map(|own| {
            if scripted {
                clock.msec
            } else {
                // Rage fires faster, its recovery slower (as JA's weapon step has it).
                (weapons::primary(own).refire_ms as f32 * fighter.force.refire(clock.time_ms))
                    as u32
            }
        });
        let trigger = specials::Trigger {
            attack: pressed(buttons::ATTACK),
            alt: pressed(buttons::ALT_ATTACK),
            now: clock.time_ms,
            msec: clock.msec,
            refire_ms: refires[0],
            pace_ms: paced.unwrap_or(spec.refire_ms),
        };
        // What each hand fires this step (a Power Blast both at once).
        let fired: [Option<(specials::Shot, u8)>; 2] = if pair {
            // A scene's shot leaves the hand its clip fires it from
            // (`minds::scene_attack`, `Discharge::hand`).
            if scripted {
                fighter.shooting.next_off = fighter.last_command.weapon == 1;
            }
            let hands = pair::Hands {
                refires,
                paces: blasters.map(|own| paced.unwrap_or(weapons::primary(own).refire_ms)),
            };
            let shots = pair::pull_special(
                &mut fighter.shooting,
                (&mut fighter.weapon_wait_ms, &mut fighter.fp),
                (special, specials::cost(&base)),
                trigger,
                hands,
            );
            [0, 1].map(|hand| shots[usize::from(hand)].map(|shot| (shot, hand)))
        } else {
            let shot = specials::pull(
                &mut fighter.shooting,
                &mut fighter.weapon_wait_ms,
                &mut fighter.fp,
                special,
                specials::cost(&base),
                trigger,
            );
            if shot.is_some() {
                fighter.shooting.hands = 1 << fighter.hand.min(1);
            }
            [shot.map(|shot| (shot, fighter.hand)), None]
        };
        for (shot, hand) in fired.into_iter().flatten() {
            let mut weapon = weapon;
            let blaster = if pair {
                weapon = in_hand(&sheet, fighter, hand);
                blasters[usize::from(hand)]
            } else {
                blaster
            };
            let spec = weapons::primary(blaster);
            // A special shapes its shot; the round's penalty to hit after one
            // rides on every shot. TSL's Sniper Bonus items add 4 to a Sniper
            // Shot's to hit; its Rapid Shot Bonus ones take a point off a Rapid
            // Shot's penalty.
            if let Some(special) = shot.special {
                weapon = special.shot(weapon);
                match special {
                    specials::Special::SniperShot(_) if sheet.gear.has(gear::traits::SNIPER) => {
                        weapon.attack_bonus = weapon
                            .attack_bonus
                            .saturating_add(rules::world::shots::SNIPER_BONUS);
                    }
                    specials::Special::RapidShot(_) if sheet.gear.has(gear::traits::RAPID_SHOT) => {
                        fighter.shooting.ease(clock.time_ms);
                    }
                    _ => {}
                }
            }
            let (penalty, _) = fighter.shooting.exposed(clock.time_ms);
            weapon.attack_bonus = weapon.attack_bonus.saturating_add(penalty);
            fighter.combat_until = clock.time_ms + COMBAT_MS;
            // Attacking shows a mind-tricking character again (JA).
            fighter.force.tricked = 0;
            let anim = if blaster == crate::combat::sheet::Blaster::Pistol {
                motion::Anim::BOTH_ATTACK2
            } else {
                motion::Anim::BOTH_ATTACK3
            };
            let shown = sent.0.get(&id.0).copied().unwrap_or(player.0.torso.anim);
            motion::begin_torso(&mut player.0, &level.0.anims, anim, shown);
            let ps = &player.0;
            let eye = mind::eye(ps);
            let muzzle = hand_muzzle(ps, spec.muzzle, if pair { hand } else { 0 });
            let share = (1000 / u16::from(spec.shards.max(1))).max(1);
            // A player's shot meets the others as it saw them (I25, C18).
            let view = match avatar {
                Some(avatar) if clock.reason != StepReason::Predict => {
                    history.view_tick(clock.tick, inputs.0.view_lag(avatar.0), clock.msec)
                }
                _ => None,
            };
            let rewound = view.map(|tick| history.targets_at(tick, level.0.friendly_fire));
            let bodies = shots.met(
                id.0,
                rewound
                    .as_ref()
                    .map_or(targets.met_by(id.0), |rewound| rewound.met_by(id.0)),
            );
            let lag_ms = view.map_or(0, |tick| (clock.tick - tick) * u64::from(clock.msec));
            // A player (or a body a seat drives) shoots at what its crosshair
            // line meets, a lit blade too (as its bolts do), from the muzzle (a
            // bolt) or the eye (a hitscan): the gun is below and beside the line
            // the screen's middle is on. A creature shoots along its view.
            let (forward, ..) = math::angle_vectors(ps.view);
            let aimed_at = (avatar.is_some() || fighter.mind.driver.is_some()).then(|| {
                let from = mind::crosshair(ps);
                let end = math::mad(from, weapons::HITSCAN_RANGE, forward);
                let seen: Vec<Body> = bodies.iter().copied().chain(blades.bodies(id.0)).collect();
                let met = level.0.with(states.as_deref(), |world| {
                    Bodies::new(world, &seen).skipping(id.0.0 | BODY).trace(
                        scale(from, UNIT),
                        scale(end, UNIT),
                        &Extents::POINT,
                        SHOT,
                    )
                });
                scale(met.end, 1.0 / UNIT)
            });
            for shard in 0..spec.shards {
                // (A point not ahead of where the shot leaves, a wall at the
                // muzzle, is shot along the view.)
                let origin = if spec.speed.is_some() { muzzle } else { eye };
                let ahead = |point: &[f32; 3]| math::dot(math::sub(*point, origin), forward) > 0.0;
                let mut view = aimed_at.filter(ahead).map_or(ps.view, |point| {
                    let (pitch, yaw) = mind::aim(origin, point);
                    [pitch, yaw, 0.0]
                });
                // The first of several shards flies true; the rest, and every
                // bolt of a spraying weapon, spread.
                if spec.spread > 0.0 && (shard > 0 || spec.shards == 1) {
                    let mut spread =
                        SimRng::new(clock.tick, u64::from(id.0.0) << 8 | u64::from(shard));
                    view[0] += spread.range(-spec.spread, spec.spread);
                    view[1] += spread.range(-spec.spread, spec.spread);
                }
                // A creature's aim, wide at first and closing as it keeps a
                // clear shot (JA's NPCs' `currentAim`).
                if creature && troop.rated != 0 && !scripted {
                    let wide = crate::combat::troop::spread(troop.aim);
                    let mut off =
                        SimRng::new(clock.tick, u64::from(id.0.0) << 8 | 0x80 | u64::from(shard));
                    view[0] += off.range(-wide, wide);
                    view[1] += off.range(-wide, wide);
                }
                let (direction, ..) = math::angle_vectors(view);
                let source = Source::Weapon {
                    weapon,
                    cadence_ms: shot.cadence_ms,
                    share,
                    area: false,
                };
                match spec.speed {
                    // A hitscan: from the eye, at once.
                    None => {
                        let end = math::mad(eye, weapons::HITSCAN_RANGE, direction);
                        let trace = level.0.with(states.as_deref(), |world| {
                            Bodies::new(world, &bodies).skipping(id.0.0 | BODY).trace(
                                scale(eye, UNIT),
                                scale(end, UNIT),
                                &Extents::POINT,
                                SHOT,
                            )
                        });
                        if scripted {
                            // Its script's blow lands when it is due.
                        } else if let Some(target) =
                            trace.entity.filter(|entity| entity & BODY != 0)
                        {
                            hits.0.push(Hit {
                                attacker: id.0,
                                target: SimId(target & !BODY),
                                source,
                                point: Some(scale(trace.end, 1.0 / UNIT)),
                                direction: Some(direction),
                            });
                        } else {
                            battering.shot(trace.entity, id.0, source);
                        }
                    }
                    Some(speed) => {
                        let velocity = scale(direction, speed).map(f32::round_ties_even);
                        // It left the muzzle when the shooter saw it leave: as
                        // far on as its view lag (C18), the way there traced
                        // against the others as the shooter saw them.
                        let bolt = Missile {
                            owner: id.0,
                            weapon,
                            cadence_ms: shot.cadence_ms,
                            share,
                            base: muzzle,
                            from_ms: clock.time_ms - lag_ms,
                            velocity,
                            half_size: spec.half_size,
                            blockable: spec.blockable,
                            until_ms: clock.time_ms + weapons::BOLT_LIFE_MS,
                            lob: None,
                            held: None,
                            scripted,
                        };
                        if lag_ms > 0 {
                            let half = bolt.half_size * UNIT;
                            let extents = Extents {
                                mins: [-half; 3],
                                maxs: [half; 3],
                            };
                            let reach = bolt.at(clock.time_ms);
                            let met = level.0.with(states.as_deref(), |world| {
                                Bodies::new(world, &bodies).skipping(id.0.0 | BODY).trace(
                                    scale(muzzle, UNIT),
                                    scale(reach, UNIT),
                                    &extents,
                                    SHOT,
                                )
                            });
                            if met.hit() {
                                if let Some(target) = met.entity.filter(|entity| entity & BODY != 0)
                                {
                                    hits.0.push(Hit {
                                        attacker: id.0,
                                        target: SimId(target & !BODY),
                                        source,
                                        point: Some(scale(met.end, 1.0 / UNIT)),
                                        direction: Some(direction),
                                    });
                                } else {
                                    battering.shot(met.entity, id.0, source);
                                }
                                continue;
                            }
                        }
                        commands.spawn((Id(SimId(next_id.0)), Bolt(bolt)));
                        next_id.0 += 1;
                    }
                }
            }
        }
    }
}

/// Where a shot or a throw leaves a character standing as `ps`: `offsets`
/// units forward, right and up of its eye along its view
/// (`CalcMuzzlePoint`), snapped to whole units.
pub fn muzzle(ps: &MotionState, [f, r, u]: [f32; 3]) -> [f32; 3] {
    let (forward, right, up) = math::angle_vectors(ps.view);
    math::add(
        math::add(math::mad(mind::eye(ps), f, forward), scale(right, r)),
        scale(up, u),
    )
    .map(f32::round_ties_even)
}

/// Where a shot from `hand` (0 the right, 1 the left) leaves: the left's
/// muzzle the right's mirrored across the view (`muzzle`).
pub fn hand_muzzle(ps: &MotionState, [f, r, u]: [f32; 3], hand: u8) -> [f32; 3] {
    muzzle(ps, [f, if hand == 1 { -r } else { r }, u])
}

/// The weapon a character holds: its sheet's in the hand it has out, else
/// its main one; a battle stim's damage rides on it.
pub(super) fn held(sheet: &Sheet, fighter: &Fighter) -> crate::combat::Weapon {
    in_hand(sheet, fighter, fighter.hand)
}

/// The weapon in `hand` of `sheet` (0 or 1), else its main one, with
/// `fighter`'s battle stim's damage, what the hands' feats do to its aim
/// (two-weapon fighting, Dueling: `combat::passive`) and what TSL's Form in
/// use does to it (`combat::forms`).
pub(super) fn in_hand(sheet: &Sheet, fighter: &Fighter, hand: u8) -> crate::combat::Weapon {
    // TSL's Form in use: damage, criticals, deflection (`combat::forms`).
    let mut weapon = crate::combat::forms::weapon(sheet, sheet.held(hand));
    weapon.deflect = weapon
        .deflect
        .saturating_add(crate::combat::forms::deflection(sheet));
    weapon.damage_bonus = weapon
        .damage_bonus
        .saturating_add(items::stim_damage(&fighter.pack));
    let aim = crate::combat::passive::hand_attack(sheet, hand);
    weapon.attack_bonus = (i32::from(weapon.attack_bonus) + aim).clamp(-128, 127) as i8;
    weapon
}

/// What shots meet: the living bodies, by entity number, and the same
/// without the party's. With friendly fire off the party's shots pass
/// through the party (a delegated decision: in JA a teammate's bolt stops
/// on a teammate and does nothing, which in co-op only wastes shots and
/// blocks doorways; KOTOR's attacks never meet allies at all).
pub(super) struct Targets {
    all: Vec<Body>,
    outside: Vec<Body>,
    /// The party's ids, in order.
    party: Vec<SimId>,
    friendly_fire: FriendlyFire,
}

impl Targets {
    /// From each actor's id, movement, bulk (`world::body`), life and
    /// party membership (the players, until companions join, I27).
    pub(super) fn of<'a>(
        friendly_fire: FriendlyFire,
        actors: impl Iterator<Item = (SimId, &'a MotionState, Option<&'a Bulk>, bool, bool)>,
    ) -> Targets {
        Targets::of_bodies(
            friendly_fire,
            actors.map(|(id, player, bulk, alive, party)| {
                (id, alive.then(|| body(id, player, bulk)), party)
            }),
        )
    }

    /// From each character's id, body (if alive) and party membership.
    pub(super) fn of_bodies(
        friendly_fire: FriendlyFire,
        actors: impl Iterator<Item = (SimId, Option<Body>, bool)>,
    ) -> Targets {
        let mut targets = Targets {
            all: Vec::new(),
            outside: Vec::new(),
            party: Vec::new(),
            friendly_fire,
        };
        for (id, body, party) in actors {
            if party {
                targets.party.push(id);
            }
            if let Some(body) = body {
                targets.all.push(body);
                if !party {
                    targets.outside.push(body);
                }
            }
        }
        targets.all.sort_by_key(|body| body.entity);
        targets.outside.sort_by_key(|body| body.entity);
        targets.party.sort();
        targets
    }

    /// The bodies `shooter`'s shots meet.
    pub(super) fn met_by(&self, shooter: SimId) -> &[Body] {
        if self.friendly_fire == FriendlyFire::Off && self.party.binary_search(&shooter).is_ok() {
            &self.outside
        } else {
            &self.all
        }
    }
}

/// Whom a shot may stop on, as KOTOR's attacks reach the one they are made
/// at and nobody stands in another's line of fire: its shooter's foes, the
/// one its shooter's mind attacks, and the party's own when friendly fire
/// lets `Targets::met_by` keep them. An ally or a neutral between a shooter
/// and its target lets the bolt by (a Sith grenadier on the Leviathan shot
/// into a service droid in the way, which no shot may harm, for ever).
pub(super) struct Sides {
    /// Each character's side and its mind's target, by id.
    of: Vec<(Side, Option<SimId>)>,
    /// The pairs of ids hostile to each other now (`module::relation`, the
    /// module's table as scripts left it), sorted.
    foes: Vec<(u32, u32)>,
}

impl Sides {
    pub(super) fn of(
        module: Option<&ModuleData>,
        adjusted: &crate::module::Adjustments,
        actors: impl Iterator<Item = (Side, Option<SimId>)>,
    ) -> Sides {
        let mut of: Vec<_> = actors.collect();
        of.sort_by_key(|(side, _)| side.id);
        let standing = module.map(|module| module.repute.with(adjusted));
        let foes = of
            .iter()
            .flat_map(|(a, _)| of.iter().map(move |(b, _)| (*a, *b)))
            .filter(|(a, b)| crate::module::relation(standing, *a, *b) == Relation::Hostile)
            .map(|(a, b)| (a.id, b.id))
            .collect();
        Sides { of, foes }
    }

    pub(super) fn side(&self, id: SimId) -> Option<&(Side, Option<SimId>)> {
        let at = self
            .of
            .binary_search_by_key(&id.0, |(side, _)| side.id)
            .ok()?;
        Some(&self.of[at])
    }

    /// Of `bodies` (`Targets::met_by`'s), those `shooter`'s shot may stop on.
    pub(super) fn met(&self, shooter: SimId, bodies: &[Body]) -> Vec<Body> {
        let Some(&(from, aim)) = self.side(shooter) else {
            return bodies.to_vec();
        };
        bodies
            .iter()
            .filter(|body| {
                let target = SimId(body.entity & !BODY);
                self.side(target).is_none_or(|&(to, _)| {
                    aim == Some(target)
                        || (from.party && to.party)
                        || self.foes.binary_search(&(from.id, to.id)).is_ok()
                })
            })
            .copied()
            .collect()
    }
}

/// `Sides` of the characters `actors` holds, with what scripts changed of
/// the factions' standing.
pub(super) fn sides_of<'a>(
    level: &Level,
    scripts: &scripts::Scripts,
    actors: impl Iterator<
        Item = (
            SimId,
            Option<&'a Avatar>,
            Option<&'a Object>,
            Option<&'a PlayerSheet>,
            &'a Fighting,
        ),
    >,
) -> Sides {
    Sides::of(
        level.module.as_deref(),
        &scripts.state.reputation,
        actors.map(|(id, avatar, object, sheet, fighting)| {
            let fighter = &fighting.0;
            (
                side_of(id, avatar, fighter, &sheet_of(sheet, object)),
                fighter.target,
            )
        }),
    )
}

/// What a shot stops at: the level and bodies (not the containment's
/// player clip).
pub const SHOT: Contents = Contents(Contents::SOLID.0 | Contents::BODY.0);
