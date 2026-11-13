//! The weapon step: what a character's weapon does to its movement and
//! what movement does to its weapon.
//!
//! Before the move, the weapon has its say over the command: a saber lock,
//! a back attack, a kick or a stab down holds the body still, a lunge or a
//! kata drives it, and the moves that leap (the jump attacks, the
//! butterflies, the back flip) push and hop it themselves; an attack slows
//! the body (`saber_speed`). After the move comes the weapon step proper:
//! the knockdown's hand state and its get-ups, an attack drawing the weapon
//! and the game putting it away, changes of weapons, and then the blade's
//! own step (`saber.rs`) or the plain step for anything else. The gait asks
//! here which stance and walk a lit blade takes.
//!
//! Hits, blocks, locks and knockdowns are the server's side: they only set
//! state (the saber state's blocked, lock and hand fields) that this step
//! reacts to.

use super::anim::{
    self, Parts, is_flip, is_kata, is_kick, is_knockdown, is_leap, is_special_move, is_spin,
    play_flags,
};
use super::{Anim, Ground, Move, events};
use crate::combat::saber::{
    Block, Blocked, Change, Getup, Hand, Held, Kit, Move as SaberMove, SaberRules, SaberState,
    Style,
};
use crate::combat::{Fighter, Sheath};
use crate::input::{buttons, holds};
use crate::math;
use crate::rng::SimRng;
use crate::world::{BODY, COMBAT_MS};
use rules::weapons_in_hand::knockdown;
use rules::weapons_in_hand::moves;
use rules::weapons_in_hand::saber_speed;

use Anim::*;
use SaberMove::*;

/// The torso's ready pose with a gun out. A gun held in one hand (any
/// pistol, `Weapon::one_handed`) takes JA's Bryar pistol pose,
/// `TORSO_WEAPONREADY2`. A gun held in both hands takes JA's rifle pose,
/// `TORSO_WEAPONREADY3`.
pub fn gun_ready(one_handed: bool) -> Anim {
    if one_handed {
        TORSO_WEAPONREADY2
    } else {
        TORSO_WEAPONREADY3
    }
}

/// The torso's shot with a gun (`gun_ready`): JA's Bryar pistol shot,
/// `BOTH_ATTACK2`, for a gun held in one hand, and its rifle shot,
/// `BOTH_ATTACK3`, for one held in both.
pub fn gun_shot(one_handed: bool) -> Anim {
    if one_handed {
        BOTH_ATTACK2
    } else {
        BOTH_ATTACK3
    }
}

/// What move_body's weapon step works on: the character's fight (its saber,
/// Force pool, weapon timer and hand), what its sheet gives it, the other
/// characters its moves' checks may meet, and the stream synced rolls draw
/// from.
pub struct Arms<'a> {
    pub fighter: &'a mut Fighter,
    pub kit: &'a Kit,
    /// A player's arms (a few rules differ for players).
    pub player: bool,
    /// A seat's keys act through this body: a player's own character, or
    /// the companion its player leads.
    pub keyed: bool,
    /// Sorted by entity number.
    pub others: &'a [Other],
    pub rng: &'a mut SimRng,
    /// A creature's enemy's eye (units), where its swings from ready aim.
    pub enemy: Option<math::Vec3>,
    /// A conversation has this party member and it fights no one: its blade
    /// goes away and stays away, whatever its 8 s say.
    pub at_rest: bool,
    /// Its player's draw or holster key drew its weapon this step
    /// (`Sheath::Drawn`): a gun is raised as an attack raises one.
    pub drew: bool,
    /// The rules the blades play by (the session's, `world::Rules`).
    pub rules: SaberRules,
}

/// Another character, as a saber move's check meets it: its body's entity
/// number in traces and what its legs play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Other {
    pub entity: u32,
    pub legs: Anim,
}

/// Whether a creature's flinch leaves it be: it spins, flips (not a
/// cartwheel or an arial), rolls, lies or gets up, or plays a special.
pub fn shrugs_off_pain(ps: &super::MotionState) -> bool {
    let legs = ps.legs.anim;
    is_spin(legs)
        || is_special_move(ps.torso.anim)
        || (is_knockdown(legs) && ps.legs.timer != 0)
        || anim::is_roll(legs)
        || (is_flip(legs)
            && !matches!(
                legs,
                BOTH_ARIAL_LEFT | BOTH_ARIAL_RIGHT | BOTH_CARTWHEEL_LEFT | BOTH_CARTWHEEL_RIGHT
            ))
}

/// Whether the gait sets its walk or stand over what the legs play: a
/// saber move that does not spin, the pair's and the staff's spin attacks
/// and the fast kata (only the arms spin), a stance, a landing, or
/// (running) a pain (QUIRKS.md Q-0038, Q-0361).
pub(super) fn gait_overrides(legs: Anim, running: bool) -> bool {
    (legs.between(BOTH_A1_T__B_, BOTH_H1_S1_BR) && !is_spin(legs))
        || matches!(
            legs,
            BOTH_SPINATTACK6
                | BOTH_SPINATTACK7
                | BOTH_A1_SPECIAL
                | BOTH_SABERFAST_STANCE
                | BOTH_SABERSLOW_STANCE
                | BOTH_STAND1
                | BOTH_STAND1TO2
                | BOTH_STAND2
                | BOTH_STAND2TO1
        )
        || (running && matches!(legs, BOTH_PAIN1 | BOTH_PAIN2 | BOTH_PAIN3))
        || anim::is_landing(legs)
}

/// Whether one of the five moves that fly the body (the pair's jump
/// attack, the staff's two, the butterflies) is still in the air.
fn flying_attack(current: SaberMove) -> bool {
    matches!(
        current,
        JUMPATTACK_DUAL
            | JUMPATTACK_STAFF_LEFT
            | JUMPATTACK_STAFF_RIGHT
            | BUTTERFLY_LEFT
            | BUTTERFLY_RIGHT
    )
}

impl Move<'_, '_> {
    /// The saber state, when the step has arms.
    fn saber(&self) -> Option<&SaberState> {
        self.arms.as_deref().map(|arms| &arms.fighter.saber)
    }

    /// Down or getting up, by the saber state's hand.
    pub(super) fn floored(&self) -> bool {
        self.saber().is_some_and(SaberState::floored)
    }

    /// The legs lie or get up and their timer still runs.
    pub(super) fn floor_anim_held(&self) -> bool {
        is_knockdown(self.ps.legs.anim) && self.ps.legs.timer != 0
    }

    fn still(&mut self) {
        self.cmd.forward = 0;
        self.cmd.right = 0;
        self.cmd.up = 0;
    }

    /// The weapon's say over the keys before the move: whether the view is
    /// held where it is.
    pub(super) fn weapon_keys(&mut self) -> bool {
        let Some(arms) = self.arms.as_deref() else {
            return false;
        };
        let saber = arms.fighter.saber;
        let gripped = arms.fighter.force.gripped_by.is_some();
        let current = saber.current_move;
        let (legs, torso) = (self.ps.legs, self.ps.torso);

        if gripped {
            self.cmd.buttons &= !(buttons::ATTACK | buttons::ALT_ATTACK);
        }

        let mut still = false;
        let mut hold_view = false;
        if saber.lock_until >= self.time_ms {
            // At or after: a kept quirk against the blade step's strict
            // test (COMMON.md).
            still = true;
            hold_view = true;
        } else if matches!(
            current,
            A_BACK | A_BACK_CR | A_BACKSTAB | A_FLIP_STAB | A_FLIP_SLASH | A_JUMP_T__B_
        ) || matches!(current, DUAL_FB | DUAL_LR)
        {
            still = true;
            let turning = moves::FLIP_TURN[0] < legs.timer && legs.timer < moves::FLIP_TURN[1];
            if matches!(legs.anim, BOTH_JUMPFLIPSTABDOWN | BOTH_JUMPFLIPSLASHDOWN1) && turning {
                self.ps.view[1] += self.seconds * moves::FLIP_TURN_RATE;
                hold_view = true;
            }
        } else if matches!(
            legs.anim,
            BOTH_A2_STABBACK1
                | BOTH_ATTACK_BACK
                | BOTH_CROUCHATTACKBACK1
                | BOTH_FORCELEAP2_T__B_
                | BOTH_JUMPFLIPSLASHDOWN1
                | BOTH_JUMPFLIPSTABDOWN
        ) {
            still = true;
        } else if legs.anim == BOTH_ROLL_STAB {
            still = true;
            hold_view = true;
        } else if current.kick() || is_kick(legs.anim) {
            still = true;
        } else if matches!(current, STABDOWN | STABDOWN_DUAL | STABDOWN_STAFF) {
            if legs.timer < moves::STAB_DOWN_STILL {
                still = true;
                hold_view = true;
            } else {
                // Creeping onto the one lying there.
                self.cmd.right = 0;
                self.cmd.up = 0;
                self.cmd.forward = moves::CREEP;
            }
        } else if matches!(current, PULL_ATTACK_STAB | PULL_ATTACK_SWING) {
            still = true;
        } else if current.kata() || is_kata(torso.anim) || is_kata(legs.anim) {
            self.kata_keys(current);
        }

        match current {
            A_LUNGE => {
                self.cmd.right = 0;
                self.cmd.up = 0;
                self.cmd.forward = if self.ps.legs.timer > moves::LUNGE_DRIVE {
                    127
                } else {
                    0
                };
            }
            A_JUMP_T__B_ => {
                if self.ps.ground.is_some() {
                    self.cmd.forward = 0;
                }
                self.cmd.right = 0;
                self.cmd.up = 0;
            }
            _ => {}
        }
        if still {
            self.still();
        }
        hold_view
            || matches!(
                current,
                A_BACK | A_BACK_CR | A_BACKSTAB | A_JUMP_T__B_ | A_LUNGE
            )
    }

    /// A kata's keys: its steps forward (and the staff's roll and hop),
    /// nothing else; the fast kata's alone are left be, its legs walking
    /// while the arms play on (JA single player; QUIRKS.md Q-0361).
    fn kata_keys(&mut self, current: SaberMove) {
        if current == A1_SPECIAL {
            return;
        }
        let t = self.ps.legs.timer;
        let inside = |(low, high): (i32, i32)| low < t && t < high;
        match self.ps.legs.anim {
            BOTH_A7_SOULCAL if current == STAFF_SOULCAL => {
                self.cmd.up = 0;
                self.cmd.right = 0;
                if inside((moves::SOULCAL_ROLL[0], moves::SOULCAL_ROLL[1])) {
                    self.cmd.up = -127;
                    self.cmd.forward = self.cmd.forward.max(0);
                } else if t >= moves::SOULCAL_CREEP {
                    self.cmd.forward = moves::CREEP;
                } else {
                    self.cmd.forward = 0;
                }
                let hop = moves::SOULCAL_HOP[0] <= t && t < moves::SOULCAL_HOP[1];
                if hop && self.ps.ground.is_some() {
                    self.hop(moves::HOP);
                }
            }
            BOTH_A2_SPECIAL => {
                let steps = moves::MEDIUM_KATA_STEPS;
                let stepping = inside((steps[0], steps[1])) || inside((steps[2], steps[3]));
                self.cmd.right = 0;
                self.cmd.up = 0;
                self.cmd.forward = if stepping { 127 } else { 0 };
            }
            BOTH_A3_SPECIAL => {
                let step = moves::STRONG_KATA_STEP;
                self.cmd.right = 0;
                self.cmd.up = 0;
                self.cmd.forward = if inside((step[0], step[1])) { 127 } else { 0 };
            }
            _ => self.still(),
        }
    }

    /// The body springs up at `speed` (units/s) from where it stands.
    fn hop(&mut self, speed: f32) {
        self.ps.velocity[2] = speed;
        self.set_jump_start(self.ps.origin[2]);
        self.ps.events |= events::JUMP;
    }

    /// The moves that drive the body themselves (the flying attacks, the
    /// back flip, the spins) take its keys: whether the view is held.
    pub(super) fn special_move_keys(&mut self) -> bool {
        let Some(arms) = self.arms.as_deref() else {
            return false;
        };
        let current = arms.fighter.saber.current_move;
        let legs = self.ps.legs.anim;
        let t = self.ps.legs.timer;
        let table = self.animator.table;
        let played = |anim: Anim| table.get(anim).length_ms() - t;

        if flying_attack(current) && legs == current.data().anim {
            self.still();
            let grounded = self.ps.ground.is_some();
            if legs == BOTH_JUMPATTACK6 {
                let p = played(BOTH_JUMPATTACK6);
                if t >= moves::JUMP_ATTACK_PUSH[0] && p >= moves::JUMP_ATTACK_PUSH[1] {
                    self.cmd.forward = 127;
                }
                let w = moves::DUAL_HOP_WINDOWS;
                let window = (t >= w[0] && p >= w[1]) || (t >= w[2] && p >= w[3]);
                let on_someone = matches!(self.ps.ground, Some(Ground::Entity(n)) if n & BODY != 0);
                if window && grounded && !on_someone {
                    self.hop(moves::HOP);
                }
            } else {
                match legs {
                    BOTH_BUTTERFLY_LEFT | BOTH_BUTTERFLY_RIGHT => {
                        if t > moves::BUTTERFLY_PUSH {
                            self.cmd.right = if legs == BOTH_BUTTERFLY_LEFT {
                                -127
                            } else {
                                127
                            };
                        }
                    }
                    _ => {
                        let pushing = t >= moves::JUMP_ATTACK_PUSH[0]
                            && played(legs) >= moves::JUMP_ATTACK_PUSH[1];
                        if pushing {
                            self.cmd.forward = 127;
                        }
                    }
                }
                let hops = moves::BUTTERFLY_HOPS;
                let left = legs == BOTH_BUTTERFLY_LEFT;
                let (from, until) = if left {
                    (hops[0], hops[1])
                } else {
                    (hops[2], hops[3])
                };
                if from <= t && t < until && grounded {
                    self.hop(if left {
                        moves::BUTTERFLY_LEFT_HOP
                    } else {
                        moves::HOP
                    });
                }
            }
            return !grounded;
        }

        match current {
            A_BACKFLIP_ATK if legs == BOTH_JUMPATTACK7 => {
                let springs = t > moves::BACKFLIP[0]
                    && played(BOTH_JUMPATTACK7) >= moves::BACKFLIP[1]
                    && self.ps.ground.is_some();
                if springs {
                    let (back, _, _) = math::angle_vectors([0.0, self.ps.view[1] + 180.0, 0.0]);
                    self.ps.velocity = math::scale(back, moves::BACKFLIP_BACK);
                    self.hop(moves::BACKFLIP_HOP);
                }
                self.still();
                false
            }
            // The spins leave the keys be: the legs walk under the spinning
            // arms (JA single player; QUIRKS.md Q-0361).
            _ => false,
        }
    }

    /// The speed the body moves at while its weapon fights.
    pub(super) fn saber_speed(&self, speed: f32) -> f32 {
        let Some(arms) = self.arms.as_deref() else {
            return speed;
        };
        let saber = &arms.fighter.saber;
        let current = saber.current_move;
        let blade = arms.kit.blade(arms.fighter.hand).is_some();
        let backing = self.cmd.forward < 0;
        let factor = match saber.style {
            style if current.attack() && backing => match style {
                Some(Style::Fast) => saber_speed::BACKING_OFF[0],
                Some(Style::Medium | Style::Dual | Style::Staff) => saber_speed::BACKING_OFF[1],
                Some(Style::Strong) => saber_speed::BACKING_OFF[2],
                None => 1.0,
            },
            Some(Style::Strong) if is_spin(self.ps.legs.anim) => saber_speed::SPINNING[1],
            _ if is_spin(self.ps.legs.anim) => saber_speed::SPINNING[0],
            style if blade && current.attack() => match style {
                Some(Style::Medium | Style::Dual | Style::Staff) => saber_speed::ATTACKING[0],
                Some(Style::Strong) => saber_speed::ATTACKING[1],
                _ => 1.0,
            },
            Some(Style::Strong) if blade && current.transition() => {
                saber_speed::STRONG_TRANSITION[usize::from(backing)]
            }
            _ => 1.0,
        };
        speed * factor
    }

    /// The saber state and whether it is a pair, when the hand holds a lit
    /// blade.
    fn lit(&self) -> Option<(&SaberState, bool)> {
        let arms = self.arms.as_deref()?;
        let hand = arms.fighter.hand;
        let saber = &arms.fighter.saber;
        (arms.kit.blade(hand).is_some() && saber.on)
            .then(|| (saber, arms.kit.forced_style(hand) == Some(Style::Dual)))
    }

    /// The stance a blade stands in: unlit the plain stand, a pair with
    /// both blades on the pair's, else the style's.
    pub(super) fn blade_stance(saber: &SaberState, pair: bool) -> Anim {
        if !saber.on {
            return BOTH_STAND1;
        }
        if pair && !saber.second_off {
            return BOTH_SABERDUAL_STANCE;
        }
        match saber.style {
            Some(Style::Dual) => BOTH_SABERDUAL_STANCE,
            Some(Style::Staff) => BOTH_SABERSTAFF_STANCE,
            Some(Style::Fast) => BOTH_SABERFAST_STANCE,
            Some(Style::Strong) => BOTH_SABERSLOW_STANCE,
            Some(Style::Medium) | None => BOTH_STAND2,
        }
    }

    /// The stance a lit blade in hand stands in.
    pub(super) fn saber_stand(&self) -> Option<Anim> {
        self.lit()
            .map(|(saber, pair)| Self::blade_stance(saber, pair))
    }

    /// The walk or run a lit blade in hand takes, by its style.
    pub(super) fn saber_gait(&self, walking: bool, backward: bool) -> Option<Anim> {
        let (saber, _) = self.lit()?;
        let style = saber.style?;
        if backward && !walking {
            return Some(BOTH_RUNBACK2);
        }
        let (run, walk, walk_back) = match style {
            Style::Staff => (BOTH_RUN_STAFF, BOTH_WALK_STAFF, BOTH_WALKBACK_STAFF),
            Style::Dual => (BOTH_RUN_DUAL, BOTH_WALK_DUAL, BOTH_WALKBACK_DUAL),
            Style::Fast | Style::Medium | Style::Strong => (BOTH_RUN2, BOTH_WALK2, BOTH_WALKBACK2),
        };
        Some(match (walking, backward) {
            (false, _) => run,
            (true, false) => walk,
            (true, true) => walk_back,
        })
    }

    /// The staff's spin attack plays.
    pub(super) fn spinning_attack(&self) -> bool {
        self.saber()
            .is_some_and(|saber| saber.current_move == SPINATTACK)
    }

    /// The weapon step after the move. The arms are taken out of the step
    /// state while it runs, and put back at its end.
    pub(super) fn weapon_step(&mut self) {
        let Some(arms) = self.arms.take() else {
            self.torso_follows_legs();
            return;
        };
        self.armed_step(arms);
        self.arms = Some(arms);
    }

    fn armed_step(&mut self, arms: &mut Arms) {
        let now = self.time_ms;
        // A left hand emptied some other way than a change of hands (its
        // weapon taken off in the equip screen, a script's unequip) gives
        // the fight back to the right hand. Left out, the right hand's gun
        // fired from the empty hand: with no shot played and its timer run
        // down twice a step, both here and by the trigger (issue 356).
        if arms.fighter.hand == 1 && arms.kit.held(1) == Held::Nothing {
            arms.fighter.hand = 0;
        }
        let hand = arms.fighter.hand;
        let held = arms.kit.held(hand);
        let gun = matches!(held, Held::Blaster(_));

        if arms.fighter.force.gripped_by.is_some()
            && arms.kit.blade(hand).is_some_and(|blade| blade.saber)
        {
            arms.fighter.saber.on = false;
        }
        // Held only while the blade's step finds it may be
        // (`saber::duel_block`); never under JA, whose rules may have
        // replaced a duel mode's since.
        let state = &mut arms.fighter.saber.duel;
        state.block_held = false;
        if arms.fighter.force.hand_out(now) || self.hand_state(arms) {
            // The keys still count while the hands are busy: a kick or an
            // aimed block wants a fresh press once they are free.
            let (kick, attack) = (self.cmd.holds & holds::KICK != 0, self.attack_held());
            let state = &mut arms.fighter.saber.duel;
            state.kick_held = kick;
            state.aimed &= attack;
            return;
        }
        if arms.rules.duel() && self.duel_kick(arms) {
            return;
        }

        // An attack draws the weapon and keeps it out. A gun put away is
        // raised first, and so is one its player's key drew this step. A
        // weapon its player drew stays `Sheath::Drawn`.
        let attack = self.cmd.buttons & (buttons::ATTACK | buttons::ALT_ATTACK) != 0;
        if gun && (arms.drew || (attack && arms.fighter.weapon_away(now))) {
            self.animator.play(
                self.ps,
                Parts::UPPER,
                TORSO_RAISEWEAP1,
                play_flags::OVER_ANY,
            );
            let timer = arms.fighter.weapon_wait_ms.max(0);
            arms.fighter.weapon_wait_ms = timer + rules::weapons_in_hand::change::RAISE_MS;
        }
        if attack {
            arms.fighter.saber.out_until = now + COMBAT_MS;
            if matches!(arms.fighter.sheath, Sheath::Holstered | Sheath::Holstering) {
                arms.fighter.sheath = Sheath::Fight;
            }
        }

        // The game lowers a gun once the fight has been over for a step.
        let until = arms.fighter.combat_until.max(arms.fighter.saber.out_until);
        let lapsed = until > 0 && until <= now && until + self.ms as u64 > now;
        if gun && arms.fighter.sheath == Sheath::Fight && arms.fighter.target.is_none() && lapsed {
            self.lower_gun(arms);
        }

        // Holstering what is not a lit blade is done at once.
        let lit_blade = arms.kit.blade(hand).is_some() && arms.fighter.saber.on;
        if arms.fighter.sheath == Sheath::Holstering && !lit_blade {
            if gun {
                self.lower_gun(arms);
            }
            arms.fighter.sheath = Sheath::Holstered;
        }

        let legs = self.ps.legs.anim;
        if is_leap(legs) || anim::roll_playing(self.ps, legs) || anim::roll_spent(self.ps, legs) {
            arms.fighter.weapon_wait_ms = arms.fighter.weapon_wait_ms.max(self.ps.legs.timer);
        }

        let Some(blade) = arms.kit.blade(hand) else {
            self.plain_step(arms);
            return;
        };
        if !matches!(arms.fighter.saber.current_move, READY | NONE) {
            // No change of weapons mid-move.
            self.cmd.weapon = hand;
        }
        if blade.saber && self.goes_dark(arms) {
            // The blade goes off with no put-away flourish, and the hilt
            // stays in hand. It counts as the player's draw only on a body
            // a seat's keys act through. On any other body, such as a
            // creature a script put in Stealth Mode, it stays a fight's.
            arms.fighter.saber.on = false;
            if arms.keyed {
                arms.fighter.sheath = Sheath::Drawn;
            }
        }
        if self.sheathes(arms) {
            self.put_away(arms);
        }
        if self.draws(arms, blade.saber) {
            if blade.saber && arms.fighter.covert.stealth && arms.fighter.sheath == Sheath::Drawn {
                // The attack that ends Stealth Mode swings at once.
                self.ignite(arms);
            } else {
                self.light(arms);
            }
        }
        self.saber_step(arms, blade);
    }

    /// The duel modes' kick key (`holds::KICK`, `docs/SABER-RULES.md` 9.2):
    /// on a press, standing free on the ground, a kick the way the keys
    /// point (ahead without one), with any weapon in hand: a lit blade
    /// kicks as its move, anything else plays the kick on the whole body.
    /// Whether it kicked.
    fn duel_kick(&mut self, arms: &mut Arms) -> bool {
        let held = self.cmd.holds & holds::KICK != 0;
        let saber = &mut arms.fighter.saber;
        let pressed = held && !saber.duel.kick_held;
        saber.duel.kick_held = held;
        let busy = arms.fighter.weapon_wait_ms > 0
            || is_kick(self.ps.legs.anim)
            || is_kick(self.ps.torso.anim)
            || saber.lock_until > self.time_ms
            || saber.floored()
            || self.ps.ground.is_none();
        if !pressed || busy {
            return false;
        }
        let kick = match (self.cmd.forward.signum(), self.cmd.right.signum()) {
            (_, 1) => SaberMove::KICK_R,
            (_, -1) => SaberMove::KICK_L,
            (-1, _) => SaberMove::KICK_B,
            _ => SaberMove::KICK_F,
        };
        let lit = arms.kit.blade(arms.fighter.hand).is_some() && arms.fighter.saber.on;
        if lit {
            self.begin_saber_move(arms, kick);
            arms.fighter.weapon_wait_ms = self.ps.torso.timer;
        } else {
            let set = play_flags::OVER_ANY | play_flags::HOLD_FULL;
            self.animator
                .play(self.ps, Parts::WHOLE, kick.data().anim, set);
            arms.fighter.weapon_wait_ms = self.ps.legs.timer;
        }
        self.still();
        true
    }

    /// The gun goes down: its lowering played and held.
    fn lower_gun(&mut self, arms: &mut Arms) {
        self.animator.play(
            self.ps,
            Parts::UPPER,
            TORSO_DROPWEAP1,
            play_flags::OVER_ANY | play_flags::HOLD_FULL,
        );
        let drop = rules::weapons_in_hand::change::DROP_MS;
        arms.fighter.weapon_wait_ms = arms.fighter.weapon_wait_ms.max(drop);
    }

    /// Whether the blade in hand goes away, a sword or vibroblade to its
    /// sheath, a lightsaber out: KOTOR holds a weapon out only in a fight
    /// and puts it away 8 s after the last blow (`COMBAT_MS`) unless it
    /// fights someone, here also 8 s after its own last attack at no one
    /// (`SaberState::out_until`); its player puts it away at once
    /// (`Sheath::Holstered`), a script may hold a saber lit
    /// (`Sheath::Lit`), and a conversation puts a party member's away
    /// (`Arms::at_rest`). Not while it swings or is asked to, changes
    /// weapons, throws its saber, or is down. [verify] JA has no
    /// sheathing; the unlit state is the one a melee weapon starts in.
    fn sheathes(&self, arms: &Arms) -> bool {
        let fighter = &*arms.fighter;
        let swinging = !matches!(fighter.saber.current_move, READY | NONE)
            || self.cmd.buttons & (buttons::ATTACK | buttons::ALT_ATTACK) != 0;
        fighter.saber.on
            && fighter.saber.change == Change::None
            && fighter.saber.thrown.is_none()
            && !swinging
            && (arms.at_rest || fighter.weapon_away(self.time_ms))
    }

    /// The blade in hand goes away (`sheathes`): a lightsaber out, the
    /// style's put-away flourish played as the saber turns off, nothing new
    /// until it is done.
    fn put_away(&mut self, arms: &mut Arms) {
        if arms.fighter.sheath == Sheath::Holstering {
            arms.fighter.sheath = Sheath::Holstered;
        }
        arms.fighter.saber.on = false;
        self.begin_saber_move(arms, PUTAWAY);
        if arms.fighter.saber.current_move == PUTAWAY {
            arms.fighter.weapon_wait_ms = arms.fighter.weapon_wait_ms.max(self.ps.torso.timer);
        }
    }

    /// Whether the blade in hand (a lightsaber when `saber`) comes out
    /// without an attack. KOTOR draws weapons as a fight begins, so a
    /// lightsaber in combat or fighting someone (`Sheath::Fight`) comes up
    /// lit before its first swing (an AI closing in under fire turns bolts),
    /// as one in TSL's fake combat does (a scene's staging, a flourish:
    /// `Fighter::fake_until`), unless its player put it away. Any blade its
    /// player drew (`Sheath::Drawn`) comes out at once. Not while it changes
    /// weapons, throws its saber, or is gripped, and a lightsaber not while
    /// it is kept unlit in Stealth Mode (`dark`).
    fn draws(&self, arms: &Arms, saber: bool) -> bool {
        let fighter = &*arms.fighter;
        let wanted = match fighter.sheath {
            Sheath::Fight => saber && (fighter.target.is_some() || fighter.in_combat(self.time_ms)),
            Sheath::Drawn => true,
            Sheath::Holstered | Sheath::Holstering | Sheath::Lit => false,
        };
        !arms.at_rest
            && !(saber && self.dark(arms))
            && !fighter.saber.on
            && wanted
            && fighter.saber.change == Change::None
            && fighter.saber.thrown.is_none()
            && fighter.force.gripped_by.is_none()
    }

    /// Whether a lightsaber in hand is kept unlit: the fighter is in Stealth
    /// Mode, neither attack button is down this step, and its saber stands
    /// ready or plays its draw or put-away (a weapon taken in hand by a
    /// change of weapons plays the draw). A swing that has begun keeps its
    /// blade lit.
    pub(super) fn dark(&self, arms: &Arms) -> bool {
        arms.fighter.covert.stealth
            && self.cmd.buttons & (buttons::ATTACK | buttons::ALT_ATTACK) == 0
            && matches!(
                arms.fighter.saber.current_move,
                READY | NONE | DRAW | PUTAWAY
            )
    }

    /// Whether a lit blade in hand goes off as its fighter is in Stealth
    /// Mode (`dark`). A blade lit for a fight or by its player's draw goes
    /// off. One a script holds lit (`Sheath::Lit`), one thrown, and one
    /// that changes weapons do not.
    fn goes_dark(&self, arms: &Arms) -> bool {
        let saber = &arms.fighter.saber;
        saber.on
            && matches!(arms.fighter.sheath, Sheath::Fight | Sheath::Drawn)
            && saber.change == Change::None
            && saber.thrown.is_none()
            && self.dark(arms)
    }

    /// The knockdown's hand state: lying, getting up, and the weapon raised
    /// once up. Whether the rest of the weapon step is skipped (still down).
    fn hand_state(&mut self, arms: &mut Arms) -> bool {
        let now = self.time_ms;
        let saber = &mut arms.fighter.saber;
        match saber.hand {
            Hand::Knockdown if saber.hand_until >= now => {
                saber.current_move = NONE;
                saber.blocking = Block::No;
                saber.blocked = Blocked::None;
                saber.change = Change::None;
                arms.fighter.weapon_wait_ms = 0;
            }
            Hand::Knockdown if saber.getup == Getup::Down => {
                self.get_up(arms);
                arms.fighter.saber.quicker = false;
            }
            Hand::Knockdown | Hand::Risen if saber.hand_until < now => {
                saber.hand = Hand::Risen;
            }
            _ => {}
        }

        let saber = &mut arms.fighter.saber;
        match saber.hand {
            Hand::None => false,
            Hand::Risen => {
                saber.change = Change::Raising;
                saber.hand = Hand::None;
                arms.fighter.weapon_wait_ms += rules::weapons_in_hand::change::RAISE_MS;
                false
            }
            Hand::Knockdown => {
                let anim = match saber.getup {
                    Getup::Down => BOTH_KNOCKDOWN1,
                    Getup::Plain => BOTH_GETUP1,
                    Getup::Force => BOTH_FORCE_GETUP_B1,
                    Getup::Quick => BOTH_FORCE_GETUP_B3,
                };
                let set = play_flags::OVER_ANY | play_flags::HOLD_FULL;
                self.animator.play(self.ps, Parts::UPPER, anim, set);
                self.ps.torso.timer = 1;
                self.animator.play(self.ps, Parts::LOWER, anim, set);
                self.ps.legs.timer = 1;
                true
            }
        }
    }

    /// Lying past the knockdown: how it gets up, by the keys as they came.
    /// From jump tier 2 the jump's get-up is the acrobatic one (QUIRKS.md
    /// Q-0043).
    fn get_up(&mut self, arms: &mut Arms) {
        let now = self.time_ms;
        let keys = self.raw;
        // Under a duel mode a fatigued player gets up plainly
        // (`docs/SABER-RULES.md` 1.4).
        let weary = arms.rules.duel() && arms.player && arms.fighter.saber.duel.fatigued;
        let saber = &mut arms.fighter.saber;
        let acrobatic = |saber: &mut SaberState, ps: &mut super::MotionState| {
            saber.getup = Getup::Quick;
            saber.hand_until = now + knockdown::FORCE_GETUP_MS;
            ps.velocity[2] = knockdown::GETUP_LIFT;
        };
        if !weary && (keys.forward != 0) != (keys.right != 0) {
            let roll = match (keys.forward, keys.right) {
                (f, _) if f > 0 => BOTH_GETUP_BROLL_F,
                (f, _) if f < 0 => BOTH_GETUP_BROLL_B,
                (_, r) if r > 0 => BOTH_GETUP_BROLL_R,
                _ => BOTH_GETUP_BROLL_L,
            };
            self.animator.play(
                self.ps,
                Parts::WHOLE,
                roll,
                play_flags::OVER_ANY | play_flags::HOLD_FULL,
            );
            saber.hand = Hand::None;
        } else if !weary && keys.up != 0 && arms.kit.force_jump > 1 {
            acrobatic(saber, self.ps);
        } else if !weary && keys.up != 0 {
            saber.getup = Getup::Force;
            saber.hand_until = now + knockdown::FORCE_GETUP_MS;
        } else if now - saber.hand_until > knockdown::LIE_MS {
            if saber.quicker {
                acrobatic(saber, self.ps);
            } else {
                saber.getup = Getup::Plain;
                saber.hand_until = now + knockdown::PLAIN_GETUP_MS;
            }
        }
    }

    /// A weapon that is not a blade (a blaster's cadence is `fire`'s): its
    /// changes of hands, whose timer runs here, and the torso when no change
    /// plays: a blaster's held up ready once its timer
    /// is out, left as it is while the timer runs (a shot plays on); fists'
    /// and a stun baton's following the legs once a punch held on it has
    /// played (`world::strikes`; JA holds its baton ready too, but `Held`
    /// does not tell a baton from fists yet).
    fn plain_step(&mut self, arms: &mut Arms) {
        // A blade taken from the hand some other way than a change of
        // hands (a script's `ActionUnequipItem`, an item given away) leaves
        // nothing lit and no move of its to finish: its put-away flourish
        // ends with it, as `raise_weapon` leaves a hand without a blade.
        let saber = &mut arms.fighter.saber;
        if saber.on || !matches!(saber.current_move, READY | NONE) {
            saber.on = false;
            saber.current_move = NONE;
            saber.blocking = Block::No;
            saber.chain = 0;
        }
        // A gun put away, by its player or the game, is not held up, and its
        // timer runs here (the trigger's runs only for a gun out).
        let ready = match arms.kit.held(arms.fighter.hand) {
            _ if arms.fighter.weapon_away(self.time_ms) => None,
            Held::Blaster(gun) => Some(gun_ready(gun.one_handed)),
            _ => None,
        };
        let changing = arms.fighter.saber.change != Change::None;
        let timed_here = changing || ready.is_none();
        if timed_here && arms.fighter.weapon_wait_ms > 0 {
            arms.fighter.weapon_wait_ms -= self.ms;
        }
        if arms.fighter.weapon_wait_ms <= 0 || changing {
            self.lower_weapon(arms);
        }
        let change = arms.fighter.saber.change;
        if arms.fighter.weapon_wait_ms > 0 {
            if change == Change::None && ready.is_none() {
                self.torso_follows_legs();
            }
            return;
        }
        match (change, ready) {
            (Change::Dropping, _) => self.raise_weapon(arms),
            (Change::Raising, Some(ready)) => {
                arms.fighter.saber.change = Change::None;
                self.ps.begin_torso(ready, self.animator.sent_torso);
            }
            (Change::None, Some(ready)) => {
                self.animator
                    .play(self.ps, Parts::UPPER, ready, play_flags::WHEN_FREE);
            }
            (Change::Raising, None) => {
                arms.fighter.saber.change = Change::None;
                self.torso_follows_legs();
            }
            (Change::None, None) => self.torso_follows_legs(),
        }
    }

    /// A change to the command's weapon starts: the one in hand is lowered.
    /// Not to the hand already held or an empty one, not while a pair of
    /// pistols is out, not twice, and not while an item is used
    /// (`Pack::busy`: it starts once the use is over).
    pub(super) fn lower_weapon(&mut self, arms: &mut Arms) {
        let wanted = self.cmd.weapon;
        let kit = arms.kit;
        let two_guns = matches!(
            (kit.held(0), kit.held(1)),
            (Held::Blaster(_), Held::Blaster(_))
        );
        if two_guns
            || arms.fighter.pack.busy()
            || wanted == arms.fighter.hand
            || kit.held(wanted) == Held::Nothing
            || arms.fighter.saber.change == Change::Dropping
        {
            return;
        }
        arms.fighter.saber.change = Change::Dropping;
        arms.fighter.weapon_wait_ms += rules::weapons_in_hand::change::DROP_MS;
        self.animator
            .play(self.ps, Parts::UPPER, TORSO_DROPWEAP1, play_flags::OVER_ANY);
    }

    /// The lowered weapon is gone: the wanted one comes up. A blade takes
    /// up its style and is drawn (a lightsaber lights on its first
    /// attack); anything else is raised. A weapon its player drew
    /// (`Sheath::Drawn`) stays drawn, so the new one is in hand.
    pub(super) fn raise_weapon(&mut self, arms: &mut Arms) {
        let wanted = self.cmd.weapon;
        if arms.fighter.sheath != Sheath::Drawn {
            arms.fighter.sheath = Sheath::Fight;
        }
        if arms.kit.held(wanted) != Held::Nothing {
            arms.fighter.hand = wanted;
        }
        let hand = arms.fighter.hand;
        let saber = &mut arms.fighter.saber;
        saber.current_move = NONE;
        saber.blocking = Block::No;
        saber.chain = 0;
        if let Some(blade) = arms.kit.blade(hand) {
            saber.style = arms.kit.style_for(hand, saber.style);
            saber.on = !blade.saber;
            self.begin_saber_move(arms, DRAW);
        } else {
            saber.on = false;
            self.animator.play(
                self.ps,
                Parts::UPPER,
                TORSO_RAISEWEAP1,
                play_flags::OVER_ANY,
            );
        }
        arms.fighter.saber.change = Change::Raising;
        arms.fighter.weapon_wait_ms += rules::weapons_in_hand::change::RAISE_MS;
    }
}
