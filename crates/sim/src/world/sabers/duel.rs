//! The duel modes in the blades' step (`saber::duel`, `docs/SABER-RULES.md`):
//! read only when the session plays one of them (`Step::rules`).
//!
//! - **Upkeep** (each tick, before the sweeps): every blade wielder's
//!   meters refill (`duel::meter::refill`), the unblockable warning, a worn
//!   hilt breaking once its swing ends, and what blows that landed on the
//!   step before give their strikers (`duel::meter::rewarded`).
//! - **Exchanges**: a blow meeting a blade, or a body that blocks it from
//!   the front (a player holding block, a creature that can), is decided
//!   once a swing a body (`duel_turned`, the swing's verdict): a lock by
//!   the setting's chance, else `duel::exchange::resolve`, whose costs,
//!   mishaps, moves and disarms are applied here.
//! - **Thrown sabers, kicks and locks** by the duel's rules, and bolts
//!   (`bolt_block`).
//!
//! Every roll is the server's stream, in id order.

use super::Hit;
use super::{Man, Step};
use crate::combat::damage::Source;
use crate::combat::fighter::{Event, Location, Outcome};
use crate::combat::mind;
use crate::combat::saber::clash::{self, roll_between};
use crate::combat::saber::defence::{self, BoltFate};
use crate::combat::saber::duel::exchange::{self, Guard, Side, Took};
use crate::combat::saber::duel::{self, Pace, meter, shots};
use crate::combat::saber::{Blocked, Kit, Move, Style, Thrown, kick, moves, power};
use crate::combat::sheet::flags;
use crate::combat::{Fighter, Sheet};
use crate::math::{self, Vec3};
use crate::motion::{self, MotionState};
use crate::rng::SimRng;
use rules::duel::block;
use rules::duel::bolts;
use rules::duel::difficulty;
use rules::duel::disarm;
use rules::duel::gains;
use rules::duel::kicks;
use rules::duel::locks;
use rules::duel::perfect;
use rules::duel::wear;

/// The heavy staggers a fighter is thrown into, one at random.
const STAGGERS: [Move; 6] = [
    Move::H1_T_,
    Move::H1_TR,
    Move::H1_TL,
    Move::H1_BL,
    Move::H1_B_,
    Move::H1_BR,
];
/// An attack's bounces, one at random (the style's own by its group).
const BOUNCES: [Move; 7] = [
    Move::B1_BL,
    Move::B1_BR,
    Move::B1_TL,
    Move::B1_TR,
    Move::B1_T_,
    Move::B1__L,
    Move::B1__R,
];
/// The knockaways a spent blocker is knocked aside into.
const ASIDE: [Move; 5] = [
    Move::K1_T_,
    Move::K1_TR,
    Move::K1_TL,
    Move::K1_BL,
    Move::K1_BR,
];
/// The stumbles of an absorbed kick on a low meter.
const STUMBLES: [motion::Anim; 3] = [
    motion::Anim::BOTH_PAIN1,
    motion::Anim::BOTH_PAIN2,
    motion::Anim::BOTH_PAIN3,
];

/// One of `list` at random.
fn pick<T: Copy>(rng: &mut SimRng, list: &[T]) -> T {
    list[roll_between(rng, 0, list.len() as i32 - 1) as usize]
}

/// A chance of `percent` in 100.
fn chance(rng: &mut SimRng, percent: i32) -> bool {
    roll_between(rng, 1, 100) <= percent
}

impl Man {
    fn pool(&self) -> u16 {
        self.kit.pool
    }

    fn is_master(&self) -> bool {
        duel::master(&self.kit, self.sheet.level)
    }

    fn mastery(&self) -> u8 {
        duel::mastery(&self.kit, self.sheet.level)
    }

    fn running(&self) -> bool {
        motion::anim::is_run(self.ps.legs.anim)
    }

    fn meter(&self) -> i32 {
        meter::meter(&self.f)
    }

    /// Its health left, in percent of its most.
    fn health(&self) -> i32 {
        self.f.hp / i32::from(self.sheet.max_hp.max(1))
    }

    /// A creature that blocks of itself (`docs/SABER-RULES.md` 10): its
    /// saber lit in hand, standing free on the ground, not in a scene, a
    /// special or a broken move, nor swinging once its mishaps reach
    /// `block::CREATURE_GUARD_UNTIL`.
    fn guards(&self, now: u64) -> bool {
        let saber = &self.f.saber;
        let mv = saber.current_move;
        !self.avatar
            && self.saber_out()
            && !self.scene
            && self.f.hp > duel::LAST_HEALTH
            && !saber.floored()
            && !saber.locked(now)
            && !mv.special()
            && !mv.broken()
            && self.ps.ground.is_some()
            && !motion::roll_playing(&self.ps, self.ps.legs.anim)
            && !(mv.attack() && saber.duel.mishap >= block::CREATURE_GUARD_UNTIL)
    }
}

impl Step<'_> {
    /// Step 4 under a duel mode: every blade wielder's duel upkeep (see the
    /// module documentation); in a scene, its meters held (`meter::scene`).
    pub(super) fn duel_upkeep(&mut self) {
        let now = self.now;
        let rewards = self.rewards();
        for (at, killed) in rewards {
            let man = &mut self.men[at];
            let player = man.avatar;
            meter::rewarded(&mut man.f, player, killed);
        }
        for at in 0..self.men.len() {
            let man = &self.men[at];
            if man.blade().is_none() || !man.f.alive() {
                continue;
            }
            if man.scene {
                let player = man.avatar;
                meter::scene(&mut self.men[at].f, player);
                continue;
            }
            let saber = &man.f.saber;
            let mv = saber.current_move;
            let rest = meter::Rest {
                player: man.avatar,
                mastery: man.mastery(),
                on_ground: man.ps.ground.is_some(),
                running: man.running(),
                crouched: man.ps.ducked(),
                floored: saber.floored(),
                locked: saber.locked(now),
                busy: mv.attack() || mv.in_between() || mv.parry() || mv.bounce() || mv.broken(),
                idle: man.f.weapon_wait_ms <= 0,
                thrown: saber.thrown.is_some(),
                shaken: mv.broken()
                    || mv.bounce()
                    || mv.knockaway()
                    || mv.parry()
                    || saber.blocked != Blocked::None,
                recovering: man.f.force.recover_until > now,
            };
            let man = &mut self.men[at];
            let pool = man.pool();
            meter::refill(&mut man.f, pool, rest, now);
            let saber = &mut man.f.saber;
            saber.duel.warn = duel::unblockable(saber.current_move);
            // A broken staff is a new weapon once put away and lit again:
            // both blades back, the style fitted to them on the next step.
            if !saber.on && saber.duel.broken {
                saber.duel.broken = false;
                saber.duel.wear = 0;
                saber.second_off = false;
            }
            let worn = saber.duel.wear >= wear::LEVELS[1];
            if worn && !saber.duel.broken && !saber.current_move.attack() {
                self.duel_break(at);
            }
        }
    }

    /// Who earns from the blows that landed on the step before: each
    /// weapon's hit on a foe, by its striker (a blade wielder, neither in a
    /// scene: KOTOR's rules pay no meter), and whether it killed.
    fn rewards(&self) -> Vec<(usize, bool)> {
        let last = self.tick.saturating_sub(1);
        let mut out = Vec::new();
        for (d, victim) in self.men.iter().enumerate() {
            for event in victim.f.events.iter().flatten() {
                let landed = matches!(
                    event.result,
                    Outcome::Hit | Outcome::Critical | Outcome::Automatic
                );
                if event.tick != last || !landed || event.weapon.is_none() {
                    continue;
                }
                let Some(at) = event.by.and_then(|by| self.find(by)) else {
                    continue;
                };
                if self.men[at].blade().is_some()
                    && self.foes(at, d)
                    && self.pair_rules(at, d).duel()
                {
                    out.push((at, !victim.f.alive()));
                }
            }
        }
        out
    }

    /// `at`'s worn staff breaks (`docs/SABER-RULES.md` 9.1): it fights on
    /// with one blade (`Duel::broken`, the weapon step), pushed back and
    /// staggered.
    fn duel_break(&mut self, at: usize) {
        let man = &mut self.men[at];
        man.f.saber.duel.broken = true;
        let (forward, ..) = math::angle_vectors([0.0, man.ps.view[1], 0.0]);
        man.ps.velocity = math::mad(man.ps.velocity, -wear::PUSH, forward);
        self.duel_took(at, Took::Stagger, None);
    }

    /// How `d` blocks a blow from `from` (`exchange::Guard`): a player by
    /// its block button, facing it; a creature that can (`Man::guards`),
    /// facing it or, from aside, by luck (`block::FALLBACK` a step of
    /// difficulty).
    pub(super) fn duel_guard(&mut self, d: usize, from: Vec3) -> Guard {
        let man = &self.men[d];
        if !man.saber_out() {
            return Guard::None;
        }
        let ahead = duel::in_front(man.ps.origin, man.ps.view[1], from);
        if man.avatar {
            return match man.f.saber.duel.guard(self.now) {
                _ if !ahead => Guard::None,
                guard => guard,
            };
        }
        if !man.guards(self.now) {
            return Guard::None;
        }
        let lucky = block::FALLBACK * difficulty::LEVEL as u32;
        if ahead || self.rng.below(1000) < lucky {
            Guard::Npc
        } else {
            Guard::None
        }
    }

    /// `at` as the exchange reads it, guarding by `guard`.
    fn duel_side(&self, at: usize, guard: Guard, defending: bool) -> Side {
        let fencer = self.fencer(at, defending);
        let man = &self.men[at];
        let mv = man.f.saber.current_move;
        let state = &man.f.saber.duel;
        Side {
            npc: !man.avatar,
            mastery: man.mastery(),
            running: man.running(),
            meter: man.meter(),
            points: i32::from(state.points),
            mishap: state.mishap,
            heavy: duel::heavy(man.style()),
            power: fencer.power,
            attacking: fencer.attacking,
            faking: state.fake,
            kata: mv.kata(),
            unblockable: duel::unblockable(mv),
            lunge: duel::lunge_or_spin(mv),
            super_break: power::final_break_won(man.ps.torso.anim),
            weary: state.weary(),
            guard,
        }
    }

    /// Whether `a`'s blow at `d` (its blade met, `met`, or its body) is
    /// stopped: decided once a swing a body, kept as the swing's verdict
    /// (as KOTOR's chance to miss is under JA's rules, `turned`). A body
    /// that does not block lets the blow on undecided.
    pub(super) fn duel_turned(&mut self, a: usize, d: usize, point: Vec3, met: bool) -> bool {
        let victim = self.men[d].id;
        if let Some(turned) = self.men[a].f.swing.verdict(victim) {
            return turned;
        }
        let from = self.men[a].ps.origin;
        // A creature parries what KOTOR's attack roll misses (`misses`),
        // as under JA's rules: the sheets decide whether its guard holds,
        // the duel how the parry plays and what it costs.
        let guard = match self.duel_guard(d, from) {
            Guard::Npc if !self.misses(a, d) => Guard::None,
            guard => guard,
        };
        let stops = (met || guard != Guard::None) && self.duel_meet(a, d, point, met, guard);
        let swing = &mut self.men[a].f.swing;
        if stops {
            swing.turn(victim, point, self.now);
        } else {
            swing.pass(victim, point, self.now);
        }
        stops
    }

    /// The exchange between `a`'s blow and `d`'s blade at `point`: a lock
    /// by the setting's chance (`locks::SETTING` in `locks::ODDS_OF`) when
    /// the exchange would try one, else its decision applied, and a lock
    /// it forces. Whether the blow is stopped.
    pub(super) fn duel_meet(
        &mut self,
        a: usize,
        d: usize,
        point: Vec3,
        met: bool,
        guard: Guard,
    ) -> bool {
        let tick = self.tick;
        let (ida, idd) = (self.men[a].id, self.men[d].id);
        let (wa, wd) = (self.men[a].delivery(), self.men[d].delivery());
        let at = if met {
            point
        } else {
            self.parried_at(d, point)
        };
        if met {
            self.men[a].record(idd, Outcome::Parried, tick, wd, point);
        }
        self.men[d].record(ida, Outcome::Parried, tick, wa, at);
        for man in [a, d] {
            self.men[man].f.combat_until = self.now + crate::world::COMBAT_MS;
        }
        let (side_a, side_d) = (
            self.duel_side(a, Guard::None, false),
            self.duel_side(d, guard, true),
        );
        if exchange::lock_chance(&side_a, &side_d)
            && roll_between(self.rng, 1, locks::ODDS_OF) <= locks::SETTING
            && !self.men[d].player_at_rest()
            && self.try_lock(a, d)
        {
            return true;
        }
        let x = exchange::resolve(&side_a, &side_d, met, self.rng);
        #[cfg(feature = "probes")]
        duel::tap::tell(duel::tap::Tapped::Exchange {
            tick,
            a: ida,
            d: idd,
            guard,
            took: x.took,
            perfect: x.perfect,
            stopped: x.stopped,
            met,
        });
        self.duel_apply(a, d, at, &x);
        if x.forced_lock && self.try_lock(a, d) {
            return true;
        }
        x.stopped
    }

    /// An exchange's costs, gains, mishaps, perfect block, wear and moves.
    fn duel_apply(&mut self, a: usize, d: usize, point: Vec3, x: &exchange::Exchange) {
        let now = self.now;
        for (i, at) in [a, d].into_iter().enumerate() {
            let man = &mut self.men[at];
            let pool = man.pool();
            meter::change(&mut man.f, x.meter[i]);
            meter::shift_points(&mut man.f.saber.duel, x.points[i]);
            meter::earn_force(&mut man.f, pool, x.force[i]);
            let state = &mut man.f.saber.duel;
            let mishaps = i32::from(state.mishap) + x.mishap[i];
            state.mishap = mishaps.clamp(0, i32::from(rules::duel::mishap::MAX)) as u8;
        }
        if x.perfect {
            self.men[d].f.saber.duel.perfect_at = now;
            if x.riposte {
                let attacker = &mut self.men[a].f;
                attacker.weapon_wait_ms = attacker.weapon_wait_ms.max(perfect::RIPOSTE_FREEZE_MS);
                attacker.saber.chain = 0;
                self.men[d].f.saber.duel.riposte_until = now + perfect::RIPOSTE_MS;
            }
        }
        let parried = matches!(
            x.took[1],
            Took::Parry | Took::WearyParry | Took::KnockedAside
        );
        if x.stopped && !x.perfect && parried {
            self.duel_wear(a, d);
        }
        self.duel_took(a, x.took[0], Some(point));
        self.duel_took(d, x.took[1], Some(point));
    }

    /// `d`'s staff worn by `a`'s blocked blow (`docs/SABER-RULES.md` 9.1):
    /// a double-bladed lightsaber's alone (KOTOR's sabers with a hilt that
    /// can break in two), not a master's, nor thrown or locked.
    fn duel_wear(&mut self, a: usize, d: usize) {
        let (att, def) = (&self.men[a], &self.men[d]);
        let staff = def.blade().is_some_and(|blade| blade.saber && blade.double);
        if !staff
            || def.is_master()
            || !meter::wears(def.avatar)
            || def.f.saber.duel.broken
            || def.f.saber.thrown.is_some()
            || def.f.saber.lock.is_some()
        {
            return;
        }
        let mv = att.f.saber.current_move;
        let special = mv.special() || duel::unblockable(mv);
        let raging = att.f.force.rage_until > self.now;
        let worn = meter::wear_of(
            att.style(),
            special,
            raging,
            i32::from(def.f.saber.duel.points),
        );
        let now = self.now;
        meter::wear(&mut self.men[d].f.saber.duel, worn, now);
    }

    /// What `at` plays after an exchange (`exchange::Took`), as the blade's
    /// step will react to it (`Blocked`): its swing bounced, an attack's
    /// bounce, a stagger, a knockaway (where the blow came in, `point`),
    /// a parry, a disarm. No stagger for one down or whose saber is away.
    fn duel_took(&mut self, at: usize, mut took: Took, point: Option<Vec3>) {
        // A disarm the guard refuses cancels the swing: it bounces.
        if took == Took::Disarm {
            if self.duel_disarm(at, gains::DISARMED) || !self.men[at].f.saber.current_move.attack()
            {
                return;
            }
            took = Took::Bounce(Pace::Blocked);
        }
        let man = &self.men[at];
        let saber = man.f.saber;
        let reeling = matches!(
            took,
            Took::HeavyStagger | Took::Stagger | Took::AttackBounce | Took::KnockedAside
        );
        if reeling && (saber.floored() || saber.thrown.is_some()) {
            return;
        }
        let guard = |man: &Man| point.map(|point| defence::parry_for(&man.ps, point, false));
        let (mv, blocked, pace) = match took {
            Took::Nothing | Took::Disarm => return,
            Took::Bounce(pace) => {
                let current = saber.current_move;
                let mv = if current.attack() {
                    moves::bounce_for(current)
                } else {
                    current
                };
                (mv, Blocked::AttackBounce, pace)
            }
            Took::AttackBounce => (pick(self.rng, &BOUNCES), Blocked::BounceMove, Pace::Blocked),
            Took::HeavyStagger | Took::Stagger => (
                pick(self.rng, &STAGGERS),
                Blocked::ParryBroken,
                Pace::Staggered,
            ),
            Took::KnockedAside => (pick(self.rng, &ASIDE), Blocked::BounceMove, Pace::Own),
            Took::Knockaway => {
                let quad = guard(man).and_then(clash::guard_of_block);
                let mv = quad.map_or(Move::K1_T_, clash::knockaway_at);
                (mv, Blocked::BounceMove, Pace::Perfect)
            }
            Took::Parry | Took::WearyParry => {
                let Some(blocked) = guard(man) else {
                    return;
                };
                (saber.current_move, blocked, Pace::Own)
            }
        };
        let saber = &mut self.men[at].f.saber;
        saber.current_move = mv;
        saber.blocked = blocked;
        saber.duel.pace = pace;
    }

    /// `at`'s saber knocked out of its hand, up (`disarm::SPEED`), unless
    /// the disarm guard refuses it (`duel::guarded`): a lightsaber in hand,
    /// not in a lock or a scene. It loses its blocks and gains `gain` block
    /// points. Whether it was disarmed.
    pub(super) fn duel_disarm(&mut self, at: usize, gain: i32) -> bool {
        let now = self.now;
        let hilt = self.placed[at][0].map(|blade| blade.now.muzzle);
        let man = &mut self.men[at];
        let lightsaber = man.blade().is_some_and(|blade| blade.saber);
        if !man.saber_out() || !lightsaber || man.f.saber.lock.is_some() || man.scene {
            return false;
        }
        if duel::guarded(&mut man.f.saber.duel, now) {
            return false;
        }
        let from = hilt.unwrap_or_else(|| mind::chest(&man.ps));
        let knocked = Thrown::knocked(from, [0.0, 0.0, disarm::SPEED], now);
        man.f.saber.thrown = Some(knocked);
        let state = &mut man.f.saber.duel;
        state.block_held = false;
        state.aimed = false;
        state.perfect_until = 0;
        meter::shift_points(state, gain);
        true
    }

    /// A thrown saber not parried by `d` under a duel mode
    /// (`docs/SABER-RULES.md` 4.7): spent, it is disarmed; else knocked
    /// aside and charged.
    pub(super) fn duel_thrown_hit(&mut self, d: usize) {
        let [under, cost] = block::THROWN;
        if self.men[d].meter() < under {
            self.duel_disarm(d, gains::THROWN_DISARM);
            return;
        }
        meter::spend(&mut self.men[d].f, cost);
        self.duel_took(d, Took::KnockedAside, None);
    }

    /// `a`'s kick landing on `d`, a saber user, under a duel mode
    /// (`docs/SABER-RULES.md` 9.2): absorbed (a player holding block,
    /// walking, facing it, the meters full enough and the mishaps few; a
    /// creature in its kick guard and not fatigued) it staggers or makes it
    /// stumble and drains a little; else it floors it with its saber out,
    /// drains more, and a creature guards against the next. The kicker
    /// gains.
    pub(super) fn duel_kicked(&mut self, a: usize, d: usize, kick: kick::Kick) {
        let now = self.now;
        let from = self.men[a].ps.origin;
        let man = &self.men[d];
        let state = man.f.saber.duel;
        let few = state.mishap < kicks::MISHAP_BELOW;
        let absorbed = if man.avatar {
            state.block_held
                && !man.running()
                && duel::in_front(man.ps.origin, man.ps.view[1], from)
                && i32::from(state.points) >= kicks::LEAST
                && meter::force(&man.f, man.pool()) >= kicks::LEAST
                && few
                && man.defence() >= 1
                && man.ps.ground.is_some()
        } else {
            state.kick_guard_until > now && !state.fatigued && few
        };
        let [absorbed_points, taken_points] = kicks::DRAINS;
        let staggers = man.meter() > kicks::STAGGER_ABOVE;
        let npc = !man.avatar;
        if absorbed {
            if staggers {
                self.duel_took(d, Took::Stagger, None);
            } else {
                let anim = pick(self.rng, &STUMBLES);
                let anims = &self.level.anims;
                motion::react(&mut self.men[d].ps, anims, anim, false);
            }
            if npc && chance(self.rng, kicks::SABER_OFF) {
                self.men[d].f.saber.on = false;
            }
        } else {
            let man = &mut self.men[d];
            let longer = kicks::DOWN_MS[usize::from(npc)];
            man.f.saber.knock_down(now, kick::DOWN_MS + longer);
            man.f.saber.on = false;
            man.ps.velocity = math::scale(kick.direction, kick.push.0);
            man.ps.velocity[2] = kick::LIFT;
            if npc {
                man.f.saber.duel.kick_guard_until = now + kicks::CREATURE_GUARD_MS;
            }
        }
        let kicker = self.men[a].id;
        let man = &mut self.men[d];
        let drain = if absorbed {
            absorbed_points
        } else {
            taken_points
        };
        meter::shift_points(&mut man.f.saber.duel, -drain);
        man.f.last_attacker = Some((kicker, now));
        let kicker = &mut self.men[a];
        meter::earn(&mut kicker.f, gains::KICK);
    }

    /// Whether a lock won with `strength` against `l` is a super break
    /// under a duel mode (`docs/SABER-RULES.md` 8.3): with executions on, a
    /// strong win and a roll under it; a master holds it to a plain break
    /// `locks::DOWNGRADE`% of the time.
    pub(super) fn duel_super_break(&mut self, l: usize, strength: i32) -> bool {
        locks::EXECUTIONS
            && strength > locks::STRENGTH_ABOVE
            && roll_between(self.rng, 0, locks::ROLLS[0]) < strength
            && !(self.men[l].is_master() && chance(self.rng, locks::DOWNGRADE))
    }

    /// A lock won by `w` over `l` with `strength` under a duel mode
    /// (`docs/SABER-RULES.md` 8.3): an execution of a weak loser, else a
    /// disarm tried; the loser held a while either way.
    pub(super) fn duel_lock_won(&mut self, w: usize, l: usize, strength: i32) {
        let [_, execute_roll, offence_roll, disarm_roll] = locks::ROLLS;
        let [weak_player, last_player, weak_npc] = locks::WEAK;
        let loser = &self.men[l];
        let health = loser.health();
        let weak = if loser.avatar {
            health <= weak_player
        } else {
            !loser.is_master() || health <= weak_npc
        };
        let executes = locks::EXECUTIONS
            && strength > locks::STRENGTH_ABOVE
            && weak
            && self.men[w].offence() > roll_between(self.rng, 1, offence_roll)
            && roll_between(self.rng, 0, execute_roll) < strength
            && (!self.men[l].avatar || health <= last_player);
        let plot = self.men[l].sheet.flags & flags::PLOT != 0;
        if executes && !plot {
            self.hits.push(Hit {
                attacker: self.men[w].id,
                target: self.men[l].id,
                source: Source::Script {
                    damage: u16::MAX,
                    damage_type: None,
                    cast: false,
                    shot: None,
                    staged: false,
                },
                point: None,
                direction: None,
            });
        } else {
            let now = self.now;
            let (winner, loser) = (&self.men[w], &self.men[l]);
            let two = matches!(loser.style(), Style::Staff | Style::Dual);
            let rage = if winner.f.force.rage_until > now {
                i32::from(winner.f.force.rage_level)
            } else {
                0
            };
            let mut odds = strength + rage;
            if two {
                odds -= locks::TWO_HANDED;
            }
            if winner.f.force.recover_until > now {
                odds -= 1;
            }
            if loser.f.force.recover_until > now {
                odds += 1;
            }
            let staff = loser.style() == Style::Staff;
            let held = staff && roll_between(self.rng, 0, 1) == 0;
            if roll_between(self.rng, 0, disarm_roll) < odds && !held {
                self.duel_disarm(l, gains::DISARMED);
            }
        }
        let loser = &mut self.men[l];
        loser.f.weapon_wait_ms = loser.ps.torso.timer + locks::STUN_MS;
    }
}

/// A bolt that met `fighter` at `at` under a duel mode
/// (`docs/SABER-RULES.md` 7), as `sabers::bolt_block` under JA's: a
/// player turns it only holding block (from any side) with the meter to
/// pay for it, a creature whenever its saber is lit in hand, paying what
/// it has; a bolt on a blade not blocking is turned wildly for
/// `bolts::WILD`. Where it goes (`shots::aim`): toward the crosshair (block
/// alone), back at the shooter's head (block and attack), a creature's at
/// the shooter's chest. Its pose by the meter left (`bolts::BANDS`).
#[allow(clippy::too_many_arguments)]
pub(in crate::world) fn bolt_block(
    player: bool,
    fighter: &mut Fighter,
    ps: &MotionState,
    sheet: &Sheet,
    bolt: &crate::combat::Missile,
    at: Vec3,
    on_blade: bool,
    shooter: Option<Vec3>,
    (now, tick): (u64, u64),
    rng: &mut SimRng,
) -> Option<BoltFate> {
    let kit = Kit::of(sheet);
    let lightsaber = kit.blade(fighter.hand).is_some_and(|blade| blade.saber);
    if !bolt.blockable
        || !fighter.alive()
        || !lightsaber
        || !fighter.saber.on
        || fighter.saber.thrown.is_some()
    {
        return None;
    }
    let state = fighter.saber.duel;
    let current = fighter.saber.current_move;
    let mastery = duel::mastery(&kit, sheet.level);
    let is_master = mastery >= rules::duel::master::DEFENSE;
    let running = motion::anim::is_run(ps.legs.anim);
    let moving = math::flat_length(ps.velocity) > 0.0;
    let blocker = shots::Blocker {
        player,
        speeding: fighter.force.speed_until > now,
        running,
        airborne: ps.ground.is_none(),
        walking: !running && moving,
        broken: current.broken(),
        kicking: current.kick(),
        pair: fighter.saber.style == Some(Style::Dual),
        held_long: state.aimed && now >= state.aimed_at + perfect::HELD_LONG_MS,
    };
    let aimed = player && state.aimed;
    let cost = meter::price(mastery, shots::cost(&blocker, aimed), true);
    let left = meter::meter(fighter);
    let steady = !(current.broken() || current.kick() || fighter.saber.floored())
        && !(running && blocker.speeding);
    let blocking = if player {
        state.block_held && steady && left >= cost
    } else {
        true
    };
    let (forward, ..) = math::angle_vectors(ps.view);
    let back = math::sub(math::scale(at, 2.0), ps.origin);
    let toward = shooter.unwrap_or(back);
    let swinging = !current.idle();
    let way = if blocking {
        meter::spend(fighter, cost.min(left));
        let aim = match (player, aimed, shooter) {
            (true, false, _) | (_, _, None) => shots::Aim::Crosshair {
                forward,
                back: fighter.last_command.forward < 0,
            },
            (true, true, Some(_)) => shots::Aim::Shooter { head: true },
            (false, _, Some(_)) => shots::Aim::Shooter { head: false },
        };
        shots::aim(aim, (at, toward), left, swinging, rng)
    } else if on_blade {
        let [wild, master_wild] = bolts::WILD;
        let wild = if is_master { master_wild } else { wild };
        meter::spend(fighter, wild.min(left));
        let mut way = if player {
            forward
        } else {
            math::sub(toward, at)
        };
        math::normalize(&mut way);
        shots::aim(shots::Aim::Wild { way }, (at, toward), left, swinging, rng)
    } else {
        return None;
    };
    if blocking && fighter.weapon_wait_ms <= 0 {
        let [aside, well] = bolts::BANDS;
        let left = meter::meter(fighter);
        if left >= aside {
            fighter.saber.blocked = defence::parry_for(ps, at, left >= well);
        }
    }
    fighter.record(Event {
        sequence: 0,
        tick,
        by: Some(bolt.owner),
        result: Outcome::Deflected,
        location: Location::None,
        damage: 0,
        weapon: Some(bolt.weapon.delivery),
        at: Some(Event::units(at)),
        visual: None,
    });
    Some(BoltFate::Turned(way))
}
