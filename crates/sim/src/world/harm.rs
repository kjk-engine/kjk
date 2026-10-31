//! The step's hits through the damage pipeline, and the effects they
//! settle.

use super::*;

/// The step's hits, in order, through the damage pipeline
/// (`combat::damage`: `assess`, then `apply`), on the authority and in
/// replays; prediction shows no damage (I22 C20) and drops them. A hit
/// pushes as JA's knockback does and holds the body with the knockback
/// timer; a death plays JA's death for where and how hard the hit was, and
/// the body goes limp, but a blow KOTOR stages (`death::in_place`: in a
/// scene, a story's blow, a round's) pushes nothing, and a death by one
/// plays KOTOR's dying clip where it stands; a
/// creature that lives may flinch (JA's NPC pain:
/// `death::pain_chance`), doing nothing new until its pain has played.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn hurt(
    clock: Res<Clock>,
    level: Res<Static>,
    mut streams: ResMut<Streams>,
    mut hits: ResMut<Hits>,
    mut heals: ResMut<Heals>,
    mut scripts: ResMut<scripts::Scripts>,
    mut battering: ResMut<objects::Battering>,
    mut notices: ResMut<crate::notice::Notices>,
    mut module: ResMut<Module>,
    (scene_held, talking): (Res<scene::Held>, Res<Talking>),
    mut states_ended: ResMut<scripts::StatesEnded>,
    benched: Query<(&Benched, &PlayerSheet)>,
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Player,
        &mut Fighting,
    )>,
) {
    let mut hits = std::mem::take(&mut hits.0);
    let mut heals = std::mem::take(&mut heals.0);
    // A test's aids (`--god`, `--mighty`) are the player's, whatever body
    // its seat drives: a benched seat's own sheet lends them to the
    // creature it plays (Canderous breaking the party out of the
    // Leviathan's cells).
    let aids = |seat: Seat| {
        benched
            .iter()
            .find(|(bench, _)| bench.0 == seat)
            .map_or(0, |(_, sheet)| {
                sheet.0.flags
                    & (crate::combat::sheet::flags::IMMORTAL | crate::combat::sheet::flags::MIGHTY)
            })
    };
    if clock.reason == StepReason::Predict {
        return;
    }
    let mut actors: Vec<_> = actors.iter_mut().collect();
    actors.sort_by_key(|(id, ..)| id.0);
    let find = |actors: &[(&Id, Option<&Avatar>, _, _, _, _)], id: SimId| {
        actors.binary_search_by_key(&id, |(own, ..)| own.0).ok()
    };
    // The effects the scripts applied last step (`script::effects`), in
    // order: damage and death as a script's hits (never filtered, C17),
    // heals with the others', FP within each one's pool; a modifier into
    // a lasting slot; a hold as C11's unless an immunity keeps it off; then
    // the lasting effects taken off.
    use crate::script::effects::Effect as ScriptEffect;
    let now = clock.time_ms;
    module.visuals.retain(|visual| visual.kept(now));
    let removed = std::mem::take(&mut scripts.state.removed);
    let remove = |actors: &mut Vec<_>, from: u32, to: u32| {
        for (object, id, _) in removed
            .iter()
            .filter(|(.., after)| (from..to).contains(after))
        {
            if let Some(at) = find(actors, SimId(*object)) {
                let row: &mut (
                    &Id,
                    Option<&Avatar>,
                    Option<&Object>,
                    Option<&PlayerSheet>,
                    Mut<Player>,
                    Mut<Fighting>,
                ) = &mut actors[at];
                effects::remove(&mut row.5.0, *id, now);
            }
        }
    };
    let applied = std::mem::take(&mut scripts.state.applied);
    let count = applied.len() as u32;
    for (index, applied) in applied.into_iter().enumerate() {
        remove(&mut actors, index as u32, index as u32 + 1);
        let (target, by) = (SimId(applied.target.0), SimId(applied.by.0));
        let at = find(&actors, target);
        // A poison decides its link (`EffectLinkEffects`): kept off where
        // it lands (Knight Valor and up, the improved resistance, a
        // Fortitude save against its DC: Affliction's average poison, DC
        // 20; Plague's virulent one, DC 100, none), nothing linked to it
        // lasts; taken, what is linked lasts as long as its course (their
        // slow).
        let leaves = applied.effect.leaves();
        let poison = leaves.iter().find_map(|leaf| match leaf {
            ScriptEffect::Poison(course) => Some(*course),
            _ => None,
        });
        let taken = poison.zip(at).and_then(|(course, at)| {
            let (.., object, sheet, _, fighting) = &mut actors[at];
            let sheet = effects::sheet_now(sheet_of(*sheet, *object), &fighting.0, now);
            let saved = course.dc > 0 && {
                let d20 = i32::try_from(streams.authority.below(rules::interaction::checks::DIE))
                    .unwrap_or(0)
                    + 1;
                crate::combat::check(d20, sheet.save(0), i32::from(course.dc))
            };
            (!fighting.0.effects.poison_immune(now) && !saved).then_some(course)
        });
        let until = match (poison, taken) {
            (None, _) => applied.until,
            (Some(_), None) => None,
            (Some(_), Some(course)) => applied
                .until
                .map(|until| until.min(now + u64::from(course.for_ms))),
        };
        for leaf in leaves {
            let mut source = None;
            match leaf {
                // `DAMAGE_TYPE_*` is a bit; bit `n` is damage type row `n`.
                ScriptEffect::Damage { amount, kind } => {
                    let row = (kind.count_ones() == 1).then(|| kind.trailing_zeros() as u8);
                    // A scene's melee blow is its attacker's weapon's (the
                    // views' hit sparks, blood and sound); a shot's bolt
                    // carries its own (`scripted_hit`).
                    let blade = find(&actors, by)
                        .filter(|_| applied.blow())
                        .map(|at| {
                            let (.., object, sheet, _, _) = &actors[at];
                            sheet_of(*sheet, *object).main_weapon().delivery
                        })
                        .filter(|delivery| !delivery.ranged());
                    source = Some(Source::Script {
                        damage: *amount,
                        damage_type: row,
                        cast: applied.cast(),
                        shot: blade,
                        staged: !applied.cast(),
                    });
                }
                // The story's death (I30 P13): a party member's too.
                ScriptEffect::Death => source = Some(Source::Death),
                ScriptEffect::Heal(amount) => heals.push((target, i32::from(*amount) * 100)),
                ScriptEffect::HealForcePoints(amount) | ScriptEffect::DamageForcePoints(amount) => {
                    let gain = matches!(leaf, ScriptEffect::HealForcePoints(_));
                    let delta = i32::from(*amount) * 100 * if gain { 1 } else { -1 };
                    if let Some(at) = at {
                        let (.., object, sheet, _, fighting) = &mut actors[at];
                        let most = i32::from(sheet_of(*sheet, *object).pool()) * 100;
                        fighting.0.fp = (fighting.0.fp + delta).clamp(0, most);
                    }
                }
                ScriptEffect::Modifier {
                    modifies,
                    amount,
                    of,
                } => {
                    if let (Some(at), Some(until)) = (at, until) {
                        let (_, avatar, object, sheet, _, fighting) = &mut actors[at];
                        let fighter = &mut fighting.0;
                        let mut of = *of;
                        match *modifies {
                            effects::Modifies::ForcePoints => {
                                fighter.fp = fighter.fp.saturating_add(i32::from(*amount) * 100);
                            }
                            effects::Modifies::Vitality => {
                                let more = i32::from(*amount) * 100;
                                fighter.pack.vitality = fighter.pack.vitality.saturating_add(more);
                            }
                            // Fury's kills are counted afresh.
                            effects::Modifies::Fury => fighter.effects.fury = Some(0),
                            // TSL's faction modifier: never on a player, only
                            // to a faction of the game's NPCs; the faction it
                            // had kept for its end, the actions it had cleared.
                            effects::Modifies::Faction => {
                                let factions =
                                    level.0.module.as_ref().map_or(0, |m| m.repute.rows.len());
                                let npc =
                                    usize::try_from(*amount).is_ok_and(|f| f > 0 && f < factions);
                                if avatar.is_some() || !npc || !fighter.alive() {
                                    continue;
                                }
                                of = faction_of(fighter, &sheet_of(*sheet, *object));
                                fighter.faction = Some(*amount as u8);
                                clear_actions(target, fighter, &mut scripts.state);
                            }
                            _ => {}
                        }
                        let lasting = effects::Lasting {
                            modifies: *modifies,
                            amount: *amount,
                            of,
                            until,
                            spell: applied.spell,
                            by: by.0,
                            id: applied.id,
                            at: now,
                        };
                        effects::add(fighter, lasting, now);
                    }
                }
                ScriptEffect::Hold(hold) => {
                    if let (Some(at), Some(until)) = (at, until) {
                        let (.., object, sheet, _, fighting) = &mut actors[at];
                        let feats = sheet_of(*sheet, *object).feats;
                        let fighter = &mut fighting.0;
                        let mind_immune = crate::combat::force::crowd::immunity::MIND_SPELLS;
                        let immune = hold.immunity().is_some_and(|kind| {
                            [kind, mind_immune]
                                .iter()
                                .any(|kind| fighter.effects.immune_to(*kind, now))
                        }) || hold.feat().is_some_and(|feat| feats.has(feat));
                        if fighter.alive()
                            && !immune
                            && fighter.force.hold(now, until.saturating_sub(now))
                        {
                            fighter.force.hold = *hold;
                        }
                    }
                }
                // A KOTOR state into a lasting slot, unless refused (its
                // immunities; a cutscene's only by sleep immunity); which
                // state holds is settled after (`settle_effects`).
                ScriptEffect::State { state, cutscene } => {
                    if let (Some(at), Some(until)) = (at, until) {
                        let (.., object, sheet, _, fighting) = &mut actors[at];
                        let feats = sheet_of(*sheet, *object).feats;
                        let fighter = &mut fighting.0;
                        let refused = effects::state::refused(
                            *state,
                            *cutscene,
                            |kind| fighter.effects.immune_effect(kind, now),
                            |feat| feats.has(feat),
                        );
                        if fighter.alive() && !refused {
                            let lasting = effects::Lasting {
                                modifies: effects::Modifies::State,
                                amount: i16::from(*cutscene),
                                of: *state,
                                until,
                                spell: applied.spell,
                                by: by.0,
                                id: applied.id,
                                at: now,
                            };
                            effects::add(fighter, lasting, now);
                        }
                    }
                }
                // A visual: an event for the views (`fighter::Visual`), on
                // the character, at its place if it was put at one; one
                // lasting kept on it too, so removing it ends it. On a door
                // or placeable, kept by the module until it ends
                // (`crate::visuals`).
                ScriptEffect::Visual {
                    row: Some(row),
                    from,
                    part,
                } => {
                    let ms = until.map_or(0, |until| {
                        until.saturating_sub(now).min(u64::from(u32::MAX)) as u32
                    });
                    let visual = crate::combat::fighter::Visual {
                        row: *row,
                        ms,
                        beam: from.is_some(),
                        part: *part,
                    };
                    if at.is_none() && applied.at.is_none() {
                        module.visuals.push(crate::visuals::OnObject {
                            id: applied.id,
                            on: target,
                            by: from.map_or(by, |from| SimId(from.0)),
                            visual,
                            since: now,
                        });
                    }
                    if let Some(at) = at {
                        if let Some(until) = until {
                            let lasting = effects::Lasting {
                                modifies: effects::Modifies::Visual,
                                amount: *row as i16,
                                of: 0,
                                until,
                                spell: applied.spell,
                                by: by.0,
                                id: applied.id,
                                at: now,
                            };
                            effects::add(&mut actors[at].5.0, lasting, now);
                        }
                        let place = applied.at.map(|p| {
                            crate::combat::fighter::Event::units(scale(p, 1.0 / collision::UNIT))
                        });
                        let by = from.map(|from| SimId(from.0)).or(Some(by));
                        actors[at].5.0.record(crate::combat::fighter::Event::visual(
                            visual, by, place, clock.tick,
                        ));
                    }
                }
                // An energy shield up for its time, one at a time (the
                // item's row for how it looks).
                ScriptEffect::Shield { types, points } => {
                    if let (Some(at), Some(until)) = (at, until) {
                        let pack = &mut actors[at].5.0.pack;
                        pack.shield = Some(crate::combat::items::Shielded {
                            row: applied.spell,
                            types: *types,
                            left: *points,
                            until,
                        });
                    }
                }
                // A poison's course, taken (above) in place of one running
                // (and its link).
                ScriptEffect::Poison(_) => {
                    if let (Some(at), Some(course)) = (at, taken) {
                        let fighter = &mut actors[at].5.0;
                        effects::end_poison(fighter, now);
                        fighter.pack.poison = Some(crate::combat::items::Poisoned::new(
                            by,
                            course,
                            now,
                            Some(applied.id),
                        ));
                    }
                }
                // A script's push: thrown down and back from its centre.
                ScriptEffect::Pushed { from } => {
                    let centre = from
                        .map(|metres| scale(metres, 1.0 / collision::UNIT))
                        .or_else(|| find(&actors, by).map(|them| actors[them].4.0.origin));
                    if let (Some(at), Some(centre)) = (at, centre) {
                        let (.., player, fighting) = &mut actors[at];
                        pushed(&mut player.0, &mut fighting.0, centre, now);
                    }
                }
                // A downed party member up again (I30 P11's rise); one dead
                // gets up too, as KOTOR's resurrection raises the dead
                // (TSL's academy: Bao-Dur spawned in his cell, killed where
                // he lies by `EffectDeath`, and raised as the cells open,
                // `a_shutdowncells`, to speak to the Exile).
                ScriptEffect::Resurrection => {
                    if let Some(at) = at {
                        let (.., player, fighting) = &mut actors[at];
                        if fighting.0.downed() || fighting.0.dead() {
                            falls::stand(&mut player.0, &mut fighting.0, &level.0.anims, now);
                        }
                    }
                }
                _ => {}
            }
            if let Some(source) = source {
                hits.push(Hit {
                    attacker: by,
                    target,
                    source,
                    point: None,
                    direction: None,
                });
            }
        }
    }
    remove(&mut actors, count, u32::MAX);
    // A mind trick or a droid's scramble breaks on a hostile its bearer
    // perceives within √10 m that is not in stealth; then what ended or
    // took hold on everyone is settled (`settle_effects`).
    let adjusted = scripts.state.reputation.clone();
    let repute = (level.0.module.as_ref()).map(|module| module.repute.with(&adjusted));
    let side = |at: usize| {
        let (id, avatar, object, sheet, _, fighting) = &actors[at];
        side_of(id.0, *avatar, &fighting.0, &sheet_of(*sheet, *object))
    };
    let alarmed: Vec<usize> = (0..actors.len())
        .filter(|&at| effects::state::fragile(actors[at].5.0.force.state))
        .filter(|&at| {
            let (id, .., player, _) = &actors[at];
            let Some(perceived) = scripts.state.perceived.get(&id.0.0) else {
                return false;
            };
            perceived.seen.iter().chain(&perceived.heard).any(|other| {
                find(&actors, SimId(*other)).is_some_and(|them| {
                    let apart = math::sub(actors[them].4.0.origin, player.0.origin);
                    let squared = math::dot(apart, apart) * collision::UNIT * collision::UNIT;
                    squared <= effects::state::FRAGILE_REACH_SQUARED
                        && !actors[them].5.0.covert.stealth
                        && crate::module::relation(repute, side(at), side(them))
                            == Relation::Hostile
                })
            })
        })
        .collect();
    for at in alarmed {
        break_fragile(&mut actors[at].5.0, now);
    }
    for (id, .., fighting) in actors.iter_mut() {
        settle_effects(
            id.0,
            &mut fighting.0,
            &mut scripts.state,
            &mut states_ended.0,
            (now, clock.tick),
        );
    }
    // Heals from others, up to each one's maximum.
    for (target, centi) in heals {
        if let Some(at) = find(&actors, target) {
            let (.., object, sheet, _, fighting) = &mut actors[at];
            let max = i32::from(sheet_of(*sheet, *object).max_hp) * 100;
            let hp = fighting.0.hp;
            if fighting.0.alive() {
                fighting.0.hp = (hp + centi).min(max).max(hp);
            }
        }
    }
    // Poison's doses due, in id order (their pain rolls draw the authority
    // stream): an effect's damage from its poisoner, never filtered; immune
    // now (Knight Valor and up, the improved resistance), it ends.
    for (id, .., fighting) in actors.iter_mut() {
        let fighter = &mut fighting.0;
        if fighter.effects.poison_immune(clock.time_ms) {
            effects::end_poison(fighter, clock.time_ms);
        }
        if fighter.alive() {
            for (points, by) in fighter.pack.doses(clock.time_ms) {
                hits.push(Hit {
                    attacker: by,
                    target: id.0,
                    source: Source::Script {
                        damage_type: None,
                        damage: u16::from(points),
                        cast: false,
                        shot: None,
                        staged: false,
                    },
                    point: None,
                    direction: None,
                });
            }
        }
    }
    if hits.is_empty() {
        return;
    }
    // The companion leading the party (`lent`) is the first seat's body as
    // well: its player's aids are its own (Bao-Dur led to the old military
    // base's force fields and security doors on Telos).
    let lent = super::lent(&scripts.state.party);
    let leader_aids = actors
        .iter()
        .find(|(_, avatar, object, ..)| {
            object.is_none() && avatar.is_some_and(|a| a.0 == LEADER_SEAT)
        })
        .and_then(|(.., sheet, _, _)| *sheet)
        .map_or(0, |sheet| {
            sheet.0.flags
                & (crate::combat::sheet::flags::IMMORTAL | crate::combat::sheet::flags::MIGHTY)
        });
    // Who each character is, as the pipeline asks it.
    let who: Vec<(Sheet, Side)> = actors
        .iter()
        .map(|(id, avatar, object, sheet, _, fighting)| {
            // What runs on it (items, the Force's KOTOR effects) changes
            // what the pipeline reads.
            let mut sheet =
                effects::sheet_now(sheet_of(*sheet, *object), &fighting.0, clock.time_ms);
            if let (Some(avatar), Some(_)) = (avatar, object) {
                sheet.flags |= aids(avatar.0);
            }
            if object.is_some() && lent == Some(id.0) {
                sheet.flags |= leader_aids;
            }
            let side = side_of(id.0, *avatar, &fighting.0, &sheet);
            (sheet, side)
        })
        .collect();
    // Attackers whose opener out of stealth a hit spent; those whose mind
    // trick a hit broke.
    let mut spent = Vec::new();
    let mut broken = Vec::new();
    for hit in hits {
        // One that found no character may have found a door or a
        // placeable (`objects`).
        let Some(at) = find(&actors, hit.target) else {
            battering.hit(&hit);
            continue;
        };
        let attacker = find(&actors, hit.attacker);
        // A weapon's hit within its attacker's opener out of stealth
        // (`stealth::OPENER_MS`): on a target that has not perceived the
        // attacker it lands unaware (S14); the opener is spent.
        let opener = attacker
            .filter(|by| *by != at && matches!(hit.source, Source::Weapon { .. }))
            .filter(|by| actors[*by].5.0.covert.opener_until >= clock.time_ms)
            .map(|by| {
                spent.push(by);
                !scripts::perceived(&scripts.state, hit.target.0, hit.attacker.0)
            });
        let attacker_seat = attacker.and_then(|by| actors[by].1.map(|avatar| avatar.0));
        let (target_sheet, target_side) = who[at];
        let target_party = target_side.party;
        let relation = match attacker {
            Some(by) => match crate::module::relation(repute, who[by].1, target_side) {
                Relation::Outsider { .. } => Relation::Outsider {
                    aimed: !who[by].1.party && actors[by].5.0.target == Some(hit.target),
                },
                relation => relation,
            },
            None => Relation::Outsider { aimed: true },
        };
        let delivery = |at: usize| held(&who[at].0, &actors[at].5.0).delivery;
        let attacker_delivery = attacker.map(delivery);
        let attacker_saber = matches!(attacker_delivery, Some(Delivery::Saber(_)));
        let attacker_blade = attacker_saber || attacker_delivery == Some(Delivery::Melee);
        let target_saber = matches!(delivery(at), Delivery::Saber(_));
        let attacker_origin = attacker.map(|by| actors[by].4.0.origin);
        // Speed's extra attacks a round scale a weapon hit's budget (C2),
        // as TSL's Juyo's does (`forms::extra_attacks`).
        let extra = attacker.map_or(0, |by| {
            let theirs = &actors[by].5.0;
            effects::extra_attacks(&theirs.force, &theirs.effects, clock.time_ms)
                + crate::combat::forms::extra_attacks(&who[by].0)
        });
        // A creature its appearance gives no movement (`moverate` NOMOVE:
        // a turret) is pushed by no blow: KOTOR moves it not at all.
        let immobile = actors[at]
            .2
            .and_then(|object| object.facts.as_ref()?.body)
            .is_some_and(|body| body.immobile());
        let (_, avatar, _, _, player, fighting) = &mut actors[at];
        // The dead take nothing more; the downed nothing but the story's
        // death.
        let death = matches!(hit.source, Source::Death);
        if !fighting.0.alive() && !(death && fighting.0.downed()) {
            continue;
        }
        let was_down = fighting.0.downed();
        // A sneak attack: a weapon's hit on one who cannot answer it.
        let sneak = match (hit.source, attacker, attacker_origin) {
            (Source::Weapon { .. }, Some(by), Some(from)) if by != at => {
                let force = &fighting.0.force;
                let exposure = specials::Exposure {
                    behind: specials::behind(player.0.origin, player.0.view[1], from),
                    held: force.stunned(clock.time_ms)
                        || force.gripped_by.is_some()
                        || fighting.0.saber.floored(),
                    unaware: opener == Some(true)
                        || avatar.is_none()
                            && (fighting.0.target.is_none()
                                || fighting.0.mind.aware_at > clock.time_ms),
                    distance: math::length(math::sub(from, player.0.origin)),
                };
                specials::sneak_dice(&who[by].0, &target_sheet, exposure)
            }
            _ => 0,
        };
        if let Some(seat) = attacker_seat.filter(|_| sneak > 0) {
            notices.0.push((
                seat,
                crate::notice::Notice::Line(locks::lines::SNEAK_ATTACK),
            ));
        }
        let ps = &mut player.0;
        let half = motion::HALF_WIDTH;
        let [x, y, z] = ps.origin;
        let location = hit.point.map_or(Location::None, |point| {
            location::locate(
                point,
                [x - half, y - half, z + motion::SOLE_Z],
                [x + half, y + half, z + ps.maxs_z],
                ps.view[1],
            )
        });
        let blow = Blow {
            attacker: Some(hit.attacker),
            relation,
            source: hit.source,
            location,
            direction: hit.direction,
            sneak,
        };
        // KOTOR's defence penalty for the round after a feat attack; the
        // immunities to criticals and sneak attacks its effects give (its
        // items' are on its sheet).
        // TSL's Form defends against its own target or another
        // (`forms::defence`).
        let targeted = fighting.0.target == Some(hit.attacker);
        let mut defending = crate::combat::forms::defending(&target_sheet, targeted);
        let (_, exposed) = fighting.0.shooting.exposed(clock.time_ms);
        defending.defence = defending.defence.saturating_add(exposed);
        for kind in [gear::immunity::CRITICAL_HIT, gear::immunity::SNEAK_ATTACK] {
            if fighting.0.effects.immune_to(kind, clock.time_ms) {
                defending.gear.immune |= 1 << kind;
            }
        }
        let assessed = damage::assess(
            &blow,
            attacker.map(|by| &who[by].0),
            &defending,
            level.0.friendly_fire,
            level.0.game(),
            &mut streams.authority,
        );
        let surprised = fighting.0.target.is_none();
        // A hit makes anyone aware at once.
        fighting.0.mind.aware_at = fighting.0.mind.aware_at.min(clock.time_ms);
        let saved = assessed.saved;
        let mut assessed = assessed;
        // Nothing stands between the story and its death.
        if !death {
            // After the pipeline, in order: speed's extra attacks scale a
            // weapon hit's budget (C2); an energy shield takes what it covers,
            // until spent; a battle stim's vitality goes before hit points;
            // protect turns its share of the rest (paid from FP); rage last.
            if extra > 0 && matches!(hit.source, Source::Weapon { .. }) {
                assessed.damage = assessed.damage.saturating_mul(1 + extra);
            }
            assessed.damage -= items::absorb(
                &mut fighting.0.pack,
                assessed.damage_type.map(usize::from),
                assessed.damage,
            );
            assessed.damage -= fighting.0.pack.soak(assessed.damage);
            let guard = fighting.0.force;
            if guard.protect_until > clock.time_ms && fighting.0.fp > 0 && assessed.damage > 0 {
                let (prevented, paid) = force::protect(
                    guard.protect_level,
                    assessed.damage,
                    target_sheet.max_hp,
                    fighting.0.fp,
                    target_sheet.max_fp,
                );
                assessed.damage -= prevented;
                fighting.0.fp -= paid;
            }
            // Rage (I23 4.6.9): a character's hit ÷ (level + 1), and it can
            // kill (QUIRKS.md Q-0044, fixed).
            let rage = guard.raging(clock.time_ms);
            if rage > 0 && attacker.is_some() && assessed.damage > 0 {
                assessed.damage = (assessed.damage / (i32::from(rage) + 1)).max(1);
            }
        }
        // A weapon's on-hit properties, rolled on a hit that landed: a
        // death takes the rest of its hit points with the blow, the rest
        // once it has landed.
        let struck = match hit.source {
            Source::Weapon { weapon, .. }
                if attacker.is_some_and(|by| by != at)
                    && assessed.outcome != crate::combat::Outcome::Refused =>
            {
                gear::on_hits(
                    &weapon,
                    &target_sheet,
                    &fighting.0,
                    clock.time_ms,
                    &mut streams.authority,
                )
            }
            _ => Vec::new(),
        };
        if struck.contains(&gear::Struck::Death) {
            assessed.damage = assessed.damage.max(fighting.0.hp);
        }
        let resolved = damage::apply(
            &blow,
            assessed,
            &target_sheet,
            &mut fighting.0,
            clock.time_ms,
            clock.tick,
            target_party,
        );
        if fighting.0.alive() {
            for effect in struck {
                struck_by(
                    &mut fighting.0,
                    effect,
                    hit.attacker,
                    clock.time_ms,
                    &level.0,
                );
            }
        }
        fighting.0.combat_until = clock.time_ms + COMBAT_MS;
        // How the attacker's blow came out, for its AI (`GetLastAttackResult`).
        if matches!(hit.source, Source::Weapon { .. }) {
            let key = (hit.attacker.0, crate::script::last::ATTACK_RESULT);
            scripts.state.last.insert(key, resolved.outcome as u32);
        }
        // A blast's secondary effect, unless a save spared it.
        if let Source::Blast { row } = hit.source
            && let Some(effect) = grenades::effect(row)
            && resolved.outcome != crate::combat::Outcome::Refused
            && !resolved.died
            && !resolved.downed
            && !(saved && effect.save_spares)
        {
            grenades::afflict(
                &mut fighting.0,
                effect.secondary,
                hit.attacker,
                clock.time_ms,
            );
        }
        // Fear ends on damage (C11).
        let fear = &mut fighting.0.force;
        if fear.hold == Hold::Fear && resolved.damage > 0 {
            fear.hold = Hold::Stun;
            fear.stunned_until = fear.stunned_until.min(clock.time_ms);
        }
        // A mind trick or a droid's scramble breaks on any damage, and on
        // another's attack reaching it.
        let attacked = attacker.is_some_and(|by| by != at)
            && matches!(hit.source, Source::Weapon { .. } | Source::Blast { .. });
        if (resolved.damage > 0 || attacked) && break_fragile(&mut fighting.0, clock.time_ms) {
            broken.push(at);
        }
        // A weapon's kill feeds its attacker's Fury.
        let killed = resolved.died && matches!(hit.source, Source::Weapon { .. });
        // A death KOTOR stages plays its dying clip where it stands.
        // KOTOR's rules: a blow it stages pushes nothing and makes no JA
        // flinch (a scene's blow plays its own reaction, `blows::HURT`).
        let scene =
            scene_held.scene(&talking.0, hit.target) || scene_held.scene(&talking.0, hit.attacker);
        let staged = death::in_place(&hit.source, scene);
        let in_place = resolved.died && !was_down && staged;
        if let Some((push, timer)) = resolved.push.filter(|_| !staged && !immobile) {
            ps.velocity = math::add(ps.velocity, push);
            if ps.hold_ms == 0 {
                ps.hold_ms = timer;
                ps.flags |= motion::motion_flags::KNOCKBACK_HOLD;
            }
        }
        if in_place {
            ps.velocity = [0.0; 3];
            fighting.0.acting = Some(death::dying(clock.time_ms));
        }
        // A fall plays a death; a downed body that dies already lies.
        if resolved.downed || (resolved.died && !was_down) {
            ps.flags |= motion::motion_flags::DEAD;
            let equivalent = resolved.damage / i32::from(target_sheet.max_hp.max(1));
            let moving = ps.velocity != [0.0; 3];
            let anim = death::pick(
                location,
                equivalent,
                moving,
                attacker_saber,
                &mut streams.authority,
            );
            motion::react(ps, &level.0.anims, anim, false);
        } else if avatar.is_none()
            && !staged
            && resolved.damage > 0
            && clock.time_ms >= fighting.0.pain_until
        {
            let chance = death::pain_chance(
                resolved.damage,
                fighting.0.hp,
                target_sheet.max_hp,
                surprised,
                attacker_blade,
                target_sheet.flags & flags::DROID != 0,
            );
            // Spins, specials, knockdowns, rolls and flips go on.
            if i64::from(streams.authority.below(1000)) < chance && !motion::shrugs_off_pain(ps) {
                let anim = death::pain(location, target_saber, &mut streams.authority);
                motion::react(ps, &level.0.anims, anim, false);
                let length = level.0.anims.get(anim).length_ms().max(0) as u64;
                fighting.0.pain_until = clock.time_ms + length;
                fighting.0.weapon_wait_ms = 0;
                // The next swing is a quick one, and the move in hand is
                // dropped (as JA's NPCs do in pain); a pair or a staff keeps its own.
                let saber = &mut fighting.0.saber;
                if saber
                    .style
                    .is_some_and(|style| !matches!(style, Style::Dual | Style::Staff))
                {
                    saber.style = Some(Style::Fast);
                    saber.current_move = SaberMove::READY;
                }
            }
        }
        if let Some(by) = attacker {
            let theirs = &mut actors[by].5.0;
            theirs.combat_until = clock.time_ms + COMBAT_MS;
            if killed && by != at {
                let effects = &mut theirs.effects;
                effects.fury = effects
                    .fury
                    .map(|kills| (kills + 1).min(effects::FURY_MOST));
            }
        }
    }
    for by in spent {
        actors[by].5.0.covert.opener_until = 0;
    }
    for at in broken {
        let (id, .., fighting) = &mut actors[at];
        settle_effects(
            id.0,
            &mut fighting.0,
            &mut scripts.state,
            &mut states_ended.0,
            (clock.time_ms, clock.tick),
        );
    }
}

/// Clears `fighter`'s (`id`'s) actions as a KOTOR state taking hold or a
/// faction changing does (`ClearAllActions`): its queue, what it plays, its
/// chase of a foe out of sight. The queue goes as a script's clear does
/// (`State::clear_queue`), its host none: a queued pause or resume names
/// the conversation it acts on (`Action::Hold::talk`).
pub(super) fn clear_actions(id: SimId, fighter: &mut Fighter, state: &mut crate::script::State) {
    let none = script_ncs::vm::ObjectId::INVALID;
    state.clear_queue(script_ncs::vm::ObjectId(id.0), [none; 2]);
    fighter.acting = None;
    fighter.mind.last_seen = None;
    fighter.mind.walker = None;
}

/// The global TSL's confusion powers set while their faction modifier
/// runs, by spell row (Force Confusion, Beast Confusion, Droid
/// Confusion), set back to 0 as it ends.
pub(super) fn confusion_global(spell: u16) -> Option<&'static str> {
    match spell {
        200 => Some("000_Human_Conf_Active"),
        184 => Some("000_Beast_Conf_Active"),
        269 => Some("000_Droid_Conf_Active"),
        _ => None,
    }
}

/// Settles `fighter`'s (`id`'s) lasting effects at `now` (step `tick`;
/// `effects::settle`): a state that ended runs its end-of-round script
/// (`ended`) where it does; one taking hold clears its actions and shows
/// its visual for its time, as a copy of its effect (its id: removing it
/// ends both); a faction modifier that ended gives the faction back, clears
/// its actions and sets its power's global to 0.
pub(super) fn settle_effects(
    id: SimId,
    fighter: &mut Fighter,
    state: &mut crate::script::State,
    ended: &mut Vec<SimId>,
    (now, tick): (u64, u64),
) {
    let settled = effects::settle(fighter, now);
    if settled.ended.is_some_and(effects::state::ends_round) {
        ended.push(id);
    }
    let mut clear = false;
    if let Some(took) = settled.took {
        clear = true;
        if let Some(row) = effects::state::visual(took.of) {
            let lasting = effects::Lasting {
                modifies: effects::Modifies::Visual,
                amount: row as i16,
                of: 0,
                at: now,
                ..took
            };
            effects::add(fighter, lasting, now);
            let ms = took.until.saturating_sub(now).min(u64::from(u32::MAX)) as u32;
            let visual = crate::combat::fighter::Visual {
                row,
                ms,
                beam: false,
                part: 0,
            };
            let by = Some(SimId(took.by));
            fighter.record(crate::combat::fighter::Event::visual(
                visual, by, None, tick,
            ));
        }
    }
    for (faction, spell) in settled.restored {
        fighter.faction = Some(faction);
        clear = true;
        if let Some(name) = confusion_global(spell) {
            crate::script::set_global(&mut state.numbers, name, 0);
        }
    }
    if clear {
        clear_actions(id, fighter, state);
    }
}

/// Ends every mind trick and droid's scramble on `fighter` at `now`
/// (`effects::state::fragile`; their visuals with them); whether there was
/// one.
pub(super) fn break_fragile(fighter: &mut Fighter, now: u64) -> bool {
    let ids: Vec<u32> = fighter
        .effects
        .running(now)
        .filter(|l| l.modifies == effects::Modifies::State && effects::state::fragile(l.of))
        .map(|l| l.id)
        .collect();
    for id in &ids {
        effects::remove(fighter, Some(*id), now);
    }
    !ids.is_empty()
}
