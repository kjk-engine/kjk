//! Grabs with bare hands and the grapple finishers (`combat::grab`), after
//! the strikes and before the hurts, on the authority and in replays: every
//! roll is the server's.
//!
//! A grab plays `BOTH_KYLE_GRAB` (`motion::Move::duel_grab`). When
//! `grab::REACH_LEFT_MS[1]` of it are left (JA's `G_GrabClient`), it reaches
//! for the victim: a living humanoid in front of the grabber within a
//! punch's reach (`strikes::punch_reach`), standing on the ground at the
//! grabber's height, not a beast or a droid, not knocked down, not in a
//! saber lock and not in a grab already; with friendly fire off, the party
//! never grabs the party. The grabber's target comes first, else the
//! nearest. The grab's roll (`grab_roll`) decides. A miss, or no one to
//! grab, plays `BOTH_KYLE_MISS`, and the grabber cannot act until it ends.
//!
//! A grab that holds picks the finisher from the grabber's movement keys:
//! none or forward the first, left or right the second, back the third.
//! Both turn to face each other, and they are moved to stand the finisher's
//! gap apart (`grab::GAP_M`) along the line between them, as JA moves its
//! pair (wp_saber.cpp:3517-3564): the grabber half the error, then the
//! victim the rest, then the grabber whatever the victim could not move,
//! each only as far as its box goes through the level. Both clips start on
//! the same step, the victim's held `grab::VICTIM_EXTRA_MS` longer, and the
//! weapon timers of both hold them, as JA's do.
//!
//! Each blow lands when the victim's clip has its moment left
//! (`grab::BLOWS_1` to `BLOWS_3`): one unarmed hit of the grabber's, with
//! its feat line, that always hits (`Source::Weapon::sure`). A blow that
//! would kill the victim leaves it at 1 hit point and marks it
//! (`damage::apply`); the blows after it deal nothing, and it dies when the
//! finisher lets it go.
//!
//! The first finisher lets the victim go with `grab::END_MS` of its clip
//! left. It throws the victim back (`grab::THROW_SPEED`) into
//! `BOTH_KNOCKDOWN2`, knocked down for `kick::DOWN_MS`. The third throws it
//! the same way into `BOTH_PLAYER_PA_3_FLY`, and a last blow lands as the
//! flight ends, when it is knocked down. The second lets go
//! `grab::LET_GO_MS` in, when the grabber's hands leave the victim's foot. It
//! always kills the victim, which falls limp from the pose it is in, and the
//! rest of the victim's clip (its stand-up) never plays. The grabber stands
//! when its own clip ends, or when the second finisher lets go.
//!
//! The grab breaks when the grabber dies or is knocked down before its clip
//! ends. It also breaks when someone else knocks the victim down, when a
//! script's death kills the victim, and when something pushes the two apart
//! by more than JA's break distance for their gap (`grab::BREAK_DIST`). Both
//! are then let go, and the victim falls. A victim a blow marked to die dies
//! then too. Any other blow that would kill a held victim, a third
//! character's too, only marks it (`damage::apply`), and the death is
//! credited to the grabber's finisher when it lets the victim go.

use bevy_ecs::prelude::*;
use collision::{Contents, Extents, UNIT};

use super::{
    Avatar, Clock, Fighting, Hit, Hits, Id, Object, Player, PlayerSheet, Static, StepReason,
    Streams, door_states, held, in_party, object_bulk, sheet_of, strikes,
};
use crate::combat::damage::{FriendlyFire, ROUND_MS, Source};
use crate::combat::grab::{Grapple, grab_roll};
use crate::combat::saber::{Move, blades, kick, sweep};
use crate::combat::sheet::{Delivery, Sheet, flags};
use crate::combat::{Fighter, Weapon, effects, items, mind};
use crate::math;
use crate::motion::{self, Anim, MotionState};
use crate::rng::SimRng;
use crate::snapshot::SimId;
use rules::duel::grab;

/// Finisher `finisher`'s clips (1 to 3), the grabber's and the victim's,
/// and when its blows land (ms of the victim's clip left).
fn clips(finisher: u8) -> (Anim, Anim, &'static [i32]) {
    match finisher {
        1 => (Anim::BOTH_KYLE_PA_1, Anim::BOTH_PLAYER_PA_1, &grab::BLOWS_1),
        2 => (Anim::BOTH_KYLE_PA_2, Anim::BOTH_PLAYER_PA_2, &grab::BLOWS_2),
        _ => (Anim::BOTH_KYLE_PA_3, Anim::BOTH_PLAYER_PA_3, &grab::BLOWS_3),
    }
}

/// The finisher the keys of `command` pick (see the module documentation).
pub(crate) fn finisher_of(forward: i8, right: i8) -> u8 {
    match (forward.signum(), right.signum()) {
        (-1, 0) => 3,
        (_, 0) => 1,
        _ => 2,
    }
}

/// How far apart (units) the finisher `finisher` (1 to 3) stands its pair.
pub fn gap_units(finisher: u8) -> f32 {
    grab::GAP_M[usize::from(finisher.clamp(1, 3) - 1)] / UNIT
}

/// A character as the grabs see it, copied out for the step and written
/// back at its end.
struct Man {
    id: SimId,
    party: bool,
    /// A beast's own strike or a model JA does not pose (`module::Bulk`):
    /// not a humanoid, never grabbed.
    beast: bool,
    personal_space: f32,
    sheet: Sheet,
    ps: MotionState,
    f: Fighter,
}

struct Step<'a> {
    level: &'a crate::Level,
    states: Option<Vec<u8>>,
    men: Vec<Man>,
    now: u64,
    msec: i32,
    rng: &'a mut SimRng,
    hits: &'a mut Vec<Hit>,
}

/// The grapples system (see the module documentation).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn grapples(
    clock: Res<Clock>,
    level: Res<Static>,
    mut hits: ResMut<Hits>,
    mut streams: ResMut<Streams>,
    scripts: Res<super::scripts::Scripts>,
    objects: Query<&Object>,
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Player,
        &mut Fighting,
    )>,
) {
    if clock.reason == StepReason::Predict {
        return;
    }
    // Nothing to do while no one grabs or is held.
    let grabbing = actors.iter().any(|(.., player, fighting)| {
        fighting.0.grapple.is_some() || player.0.torso.anim == Anim::BOTH_KYLE_GRAB
    });
    if !grabbing {
        return;
    }
    let now = clock.time_ms;
    let lent = super::lent(&scripts.state.party);
    let mut rows: Vec<_> = actors.iter_mut().collect();
    rows.sort_by_key(|(id, ..)| id.0);
    let men = rows
        .iter()
        .map(|(id, avatar, object, sheet, player, fighting)| {
            let facts = object.and_then(|object| object.facts.as_ref());
            Man {
                id: id.0,
                party: in_party(*avatar, &fighting.0) || lent == Some(id.0),
                beast: facts.is_some_and(|facts| facts.strike.is_some())
                    || object_bulk(*object).is_some(),
                personal_space: facts.map_or(0.0, |facts| facts.personal_space),
                sheet: effects::sheet_now(sheet_of(*sheet, *object), &fighting.0, now),
                ps: player.0,
                f: fighting.0,
            }
        })
        .collect();
    let mut step = Step {
        level: &level.0,
        states: door_states(&level.0, &objects),
        men,
        now,
        msec: clock.msec as i32,
        rng: &mut streams.authority,
        hits: &mut hits.0,
    };
    step.reaches();
    step.victims();
    step.grabbers();
    for (row, man) in rows.iter_mut().zip(step.men) {
        row.4.0 = man.ps;
        row.5.0 = man.f;
    }
}

impl Step<'_> {
    fn find(&self, id: SimId) -> Option<usize> {
        self.men.binary_search_by_key(&id, |man| man.id).ok()
    }

    fn length(&self, anim: Anim) -> i32 {
        self.level.anims.get(anim).length_ms().max(0)
    }

    /// Each grab whose reach comes this step: its victim and its roll.
    fn reaches(&mut self) {
        let left = grab::REACH_LEFT_MS[1];
        for g in 0..self.men.len() {
            let man = &self.men[g];
            let timer = man.ps.torso.timer;
            let reaching = man.ps.torso.anim == Anim::BOTH_KYLE_GRAB
                && timer <= left
                && timer + self.msec > left;
            if !man.f.alive() || man.f.grapple.is_some() || !reaching {
                continue;
            }
            let held = self.victim(g).filter(|&v| {
                let d20 = i64::from(self.rng.below(rules::interaction::checks::DIE)) + 1;
                let (grabber, victim) = (&self.men[g], &self.men[v]);
                grab_roll(d20, &grabber.sheet, &victim.sheet, victim.f.hp)
            });
            match held {
                Some(v) => self.begin(g, v),
                None => self.stumble(g),
            }
        }
    }

    /// Whom `g`'s grab takes, if anyone (see the module documentation).
    fn victim(&self, g: usize) -> Option<usize> {
        let grabber = &self.men[g];
        let ps = &grabber.ps;
        let reach = strikes::punch_reach(grabber.personal_space);
        let (forward, ..) = math::angle_vectors([0.0, ps.view[1], 0.0]);
        let apart = self.level.friendly_fire == FriendlyFire::Off && grabber.party;
        let flat = |man: &Man| math::sub(man.ps.origin, ps.origin);
        let takes = |man: &Man| {
            let mut way = flat(man);
            way[2] = 0.0;
            let distance = math::normalize(&mut way);
            let f = &man.f;
            man.id != grabber.id
                && f.alive()
                && !man.beast
                && man.sheet.flags & flags::DROID == 0
                && f.grapple.is_none()
                && !f.saber.floored()
                && f.saber.lock.is_none()
                && man.ps.ground.is_some()
                && !(apart && man.party)
                && distance <= reach
                && (man.ps.origin[2] - ps.origin[2]).abs() <= grab::HEIGHT_UNITS
                && (distance == 0.0 || math::dot(way, forward) >= rules::creatures::fists::FRONT)
        };
        let near = |at: &usize| math::flat_length(flat(&self.men[*at]));
        let found: Vec<usize> = (0..self.men.len())
            .filter(|&at| takes(&self.men[at]))
            .collect();
        let target = grabber.f.target.and_then(|target| self.find(target));
        found
            .iter()
            .copied()
            .find(|&at| Some(at) == target)
            .or_else(|| {
                found
                    .iter()
                    .copied()
                    .min_by(|a, b| near(a).total_cmp(&near(b)))
            })
    }

    /// `g`'s grab missed: the stumble plays, and it cannot act until it ends.
    fn stumble(&mut self, g: usize) {
        let anims = &self.level.anims;
        let man = &mut self.men[g];
        motion::react_anew(&mut man.ps, anims, Anim::BOTH_KYLE_MISS);
        man.f.weapon_wait_ms = man.ps.legs.timer.max(0);
    }

    /// `g` holds `v`: the finisher its keys pick begins on both.
    fn begin(&mut self, g: usize, v: usize) {
        let now = self.now;
        let keys = self.men[g].f.last_command;
        let finisher = finisher_of(keys.forward, keys.right);
        let (grabber_anim, victim_anim, _) = clips(finisher);
        self.face(g, v);
        self.close_in(g, v, gap_units(finisher));
        let anims = &self.level.anims;
        let ids = (self.men[g].id, self.men[v].id);
        for (at, anim, partner, grabber) in [
            (g, grabber_anim, ids.1, true),
            (v, victim_anim, ids.0, false),
        ] {
            let man = &mut self.men[at];
            motion::react_anew(&mut man.ps, anims, anim);
            if !grabber {
                man.ps.legs.timer += grab::VICTIM_EXTRA_MS;
                man.ps.torso.timer += grab::VICTIM_EXTRA_MS;
            }
            man.ps.velocity = [0.0; 3];
            let f = &mut man.f;
            f.weapon_wait_ms = man.ps.legs.timer.max(0);
            // JA puts both blades out as the pair is placed
            // (wp_saber.cpp:3168-3169).
            f.saber.on = false;
            f.saber.current_move = Move::NONE;
            f.combat_until = now + super::COMBAT_MS;
            f.grapple = Some(Grapple {
                partner,
                finisher,
                grabber,
                since: now,
                ..Grapple::BLANK
            });
        }
        self.men[v].f.last_attacker = Some((ids.0, now));
    }

    /// `a` looks straight at `b` (level), and `b` straight back.
    fn face(&mut self, a: usize, b: usize) {
        let yaw = sweep::yaw_to(self.men[a].ps.origin, self.men[b].ps.origin);
        self.men[a].ps.set_view([0.0, yaw, 0.0]);
        let back = blades::normalize_180(yaw + 180.0);
        self.men[b].ps.set_view([0.0, back, 0.0]);
    }

    /// Brings `g` and `v` to `ideal` units apart along the line between
    /// them: `g` takes half the error, `v` the rest, then `g` what `v`
    /// could not take. Each moves only as far as its box goes through the
    /// level, and not at all from inside a solid.
    fn close_in(&mut self, g: usize, v: usize, ideal: f32) {
        for (mover, other, share) in [(g, v, 0.5), (v, g, 1.0), (g, v, 1.0)] {
            let from = self.men[mover].ps.origin;
            let mut way = math::sub(self.men[other].ps.origin, from);
            way[2] = 0.0;
            let error = math::normalize(&mut way) - ideal;
            if error.abs() < 0.01 {
                continue;
            }
            let goal = math::mad(from, error * share, way);
            let extents = Extents {
                mins: [
                    -motion::HALF_WIDTH * UNIT,
                    -motion::HALF_WIDTH * UNIT,
                    motion::SOLE_Z * UNIT,
                ],
                maxs: [
                    motion::HALF_WIDTH * UNIT,
                    motion::HALF_WIDTH * UNIT,
                    self.men[mover].ps.maxs_z * UNIT,
                ],
            };
            let trace = self.level.with(self.states.as_deref(), |world| {
                world.trace(
                    math::scale(from, UNIT),
                    math::scale(goal, UNIT),
                    &extents,
                    Contents::SOLID,
                )
            });
            if !trace.start_solid && !trace.all_solid {
                self.men[mover].ps.origin = math::scale(trace.end, 1.0 / UNIT);
            }
        }
    }

    /// Each victim in a grab: its blows, its release, or a break.
    fn victims(&mut self) {
        for v in 0..self.men.len() {
            let Some(held) = self.men[v].f.grapple.filter(|held| !held.grabber) else {
                continue;
            };
            let (grabber_anim, victim_anim, blows) = clips(held.finisher);
            let length = self.length(victim_anim) + grab::VICTIM_EXTRA_MS;
            let t = self.now.saturating_sub(held.since) as i32;
            let partner = self.find(held.partner);
            let broken = if held.flying {
                !self.men[v].f.alive()
            } else {
                self.broken(v, partner, t < self.length(grabber_anim))
            };
            if broken {
                self.free(v, partner);
                continue;
            }
            let mut held = held;
            while let Some(&left) = blows.get(usize::from(held.blows)) {
                if t < (length - left).max(0) {
                    break;
                }
                self.blow(v, &mut held);
            }
            let thrown = length - grab::END_MS;
            // The second lets go as the grabber's hands leave the foot, and
            // kills.
            if held.finisher == 2 && t >= grab::LET_GO_MS.min(thrown) {
                self.let_go(v, partner, held);
                continue;
            }
            if !held.flying && t >= thrown {
                match held.finisher {
                    1 => {
                        self.throw(v, partner, Anim::BOTH_KNOCKDOWN2);
                        self.release(v, held);
                        continue;
                    }
                    3 => {
                        self.throw(v, partner, Anim::BOTH_PLAYER_PA_3_FLY);
                        held.flying = true;
                    }
                    _ => {
                        self.release(v, held);
                        continue;
                    }
                }
            }
            if held.flying {
                let flight = self.length(Anim::BOTH_PLAYER_PA_3_FLY);
                if t - thrown >= flight - grab::END_MS {
                    self.blow(v, &mut held);
                    self.release(v, held);
                    continue;
                }
            }
            self.men[v].f.grapple = Some(held);
        }
    }

    /// Whether `v`'s grab breaks (see the module documentation), its
    /// grabber at `partner` still in its clip when `holding`.
    fn broken(&self, v: usize, partner: Option<usize>, holding: bool) -> bool {
        let victim = &self.men[v];
        if !victim.f.alive() || victim.f.saber.floored() {
            return true;
        }
        if !holding {
            return false;
        }
        let Some(p) = partner else {
            return true;
        };
        let grabber = &self.men[p];
        let ours = grabber
            .f
            .grapple
            .is_some_and(|held| held.grabber && held.partner == victim.id);
        let mut apart = math::sub(grabber.ps.origin, victim.ps.origin);
        apart[2] = 0.0;
        let finisher = victim.f.grapple.map_or(1, |held| held.finisher);
        let most = gap_units(finisher) * grab::BREAK_DIST[1] / grab::BREAK_DIST[0];
        !ours || !grabber.f.alive() || grabber.f.saber.floored() || math::length(apart) > most
    }

    /// The grab on `v` broken: both let go, the victim knocked down, the
    /// grabber (at `partner`) stumbling if its clip had a way to go.
    /// A victim a blow marked to die (`Grapple::dead`) dies now.
    fn free(&mut self, v: usize, partner: Option<usize>) {
        let now = self.now;
        let held = self.men[v].f.grapple;
        let victim = &mut self.men[v];
        let id = victim.id;
        victim.f.grapple = None;
        victim.f.weapon_wait_ms = 0;
        if victim.f.alive() {
            victim.f.saber.knock_down(now, kick::DOWN_MS);
        }
        if let Some(held) = held.filter(|held| held.dead) {
            self.finish(v, held);
        }
        let Some(p) = partner else {
            return;
        };
        let grabber = &mut self.men[p];
        if grabber.f.grapple.is_some_and(|held| held.partner == id) {
            grabber.f.grapple = None;
            grabber.f.weapon_wait_ms = 0;
            if grabber.f.alive() && !grabber.f.saber.floored() {
                self.stumble(p);
            }
        }
    }

    /// The next blow of `held`'s finisher lands on `v`: one sure unarmed
    /// hit of its grabber's, nothing once the victim is marked to die.
    fn blow(&mut self, v: usize, held: &mut Grapple) {
        held.blows = held.blows.saturating_add(1);
        if held.dead {
            return;
        }
        let victim = &self.men[v];
        let grabber = self.find(held.partner).map(|p| &self.men[p]);
        let weapon = grabber.map_or(Weapon::UNARMED, |grabber| {
            let (kit, line) = strikes::feat_line(&grabber.sheet, false);
            let mut weapon = kit.carry(line, held_weapon(&grabber.sheet, &grabber.f));
            weapon.damage_bonus = weapon
                .damage_bonus
                .saturating_add(items::stim_damage(&grabber.f.pack));
            weapon
        });
        let from = grabber.map_or(victim.ps.origin, |grabber| grabber.ps.origin);
        let mut direction = math::sub(victim.ps.origin, from);
        direction[2] = 0.0;
        math::normalize(&mut direction);
        self.hits.push(Hit {
            attacker: held.partner,
            target: victim.id,
            source: Source::Weapon {
                weapon,
                cadence_ms: ROUND_MS as u32,
                share: 1000,
                area: false,
                second: None,
                sure: true,
            },
            point: Some(math::mad(
                mind::chest_of(&victim.ps, None),
                -motion::HALF_WIDTH,
                direction,
            )),
            direction: Some(direction),
        });
    }

    /// `v` thrown away from its grabber (at `partner`) into `anim`, as
    /// JA's `G_Throw` throws it (`grab::THROW`).
    fn throw(&mut self, v: usize, partner: Option<usize>, anim: Anim) {
        let from = partner.map(|p| self.men[p].ps.origin);
        let anims = &self.level.anims;
        let victim = &mut self.men[v];
        let mut away = match from {
            Some(from) => math::sub(victim.ps.origin, from),
            None => math::scale(math::angle_vectors([0.0, victim.ps.view[1], 0.0]).0, -1.0),
        };
        away[2] = 0.0;
        math::normalize(&mut away);
        let back = grab::THROW_SPEED;
        let [lift, tip] = grab::THROW;
        let (sin, cos) = collision::trig::sin_cos_deg(tip);
        let mut velocity = math::scale(away, back * cos);
        velocity[2] = lift * sin;
        motion::react_anew(&mut victim.ps, anims, anim);
        victim.ps.velocity = velocity;
        victim.f.weapon_wait_ms = victim.ps.legs.timer.max(0);
    }

    /// The finisher lets `v` go: knocked down, and killed now if a blow
    /// marked it (`held.dead`).
    fn release(&mut self, v: usize, held: Grapple) {
        let victim = &mut self.men[v];
        victim.f.grapple = None;
        if victim.f.alive() {
            victim.f.saber.knock_down(self.now, kick::DOWN_MS);
        }
        if held.dead {
            self.finish(v, held);
        }
    }

    /// The second finisher lets `v` go: it dies, and falls limp from the
    /// pose it is in (`Fighter::blasted_ms`: the view's ragdoll, the rest of
    /// its clip never played), whatever its health. Its grabber (at
    /// `partner`) stands free, its clip let go too.
    fn let_go(&mut self, v: usize, partner: Option<usize>, held: Grapple) {
        let now = self.now;
        let victim = &mut self.men[v];
        let id = victim.id;
        victim.f.grapple = None;
        victim.f.weapon_wait_ms = 0;
        if victim.f.alive() {
            victim.f.blasted_ms = now;
        }
        self.finish(v, held);
        if let Some(p) = partner {
            let grabber = &mut self.men[p];
            if grabber
                .f
                .grapple
                .is_some_and(|g| g.grabber && g.partner == id)
            {
                grabber.f.grapple = None;
                grabber.f.weapon_wait_ms = 0;
                grabber.ps.legs.timer = 0;
                grabber.ps.torso.timer = 0;
            }
        }
    }

    /// The victim at `v`, marked to die by a blow while held, dies: what it
    /// has left (1 HP, `damage::apply`) is taken by the grabber's finisher.
    fn finish(&mut self, v: usize, held: Grapple) {
        let victim = &self.men[v];
        if !victim.f.alive() {
            return;
        }
        {
            let left = u16::try_from((victim.f.hp + 99) / 100)
                .unwrap_or(u16::MAX)
                .max(1);
            self.hits.push(Hit {
                attacker: held.partner,
                target: victim.id,
                source: Source::Script {
                    damage: left,
                    damage_type: None,
                    cast: false,
                    shot: None,
                    staged: false,
                },
                point: None,
                direction: None,
            });
        }
    }

    /// Each grabber whose clip has ended stands free; one knocked down or
    /// dead lets go.
    fn grabbers(&mut self) {
        for g in 0..self.men.len() {
            let Some(held) = self.men[g].f.grapple.filter(|held| held.grabber) else {
                continue;
            };
            let (grabber_anim, ..) = clips(held.finisher);
            let t = self.now.saturating_sub(held.since) as i32;
            let man = &self.men[g];
            if t >= self.length(grabber_anim) || !man.f.alive() || man.f.saber.floored() {
                self.men[g].f.grapple = None;
            }
        }
    }
}

/// What `fighter` strikes with by `sheet`: its fists, or a creature's own
/// unarmed weapon.
fn held_weapon(sheet: &Sheet, fighter: &Fighter) -> Weapon {
    let weapon = held(sheet, fighter);
    if weapon.delivery == Delivery::Unarmed {
        weapon
    } else {
        Weapon::UNARMED
    }
}
