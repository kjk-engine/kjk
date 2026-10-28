//! The blade's step: what one movement step does with a lightsaber or a
//! melee weapon in hand.
//!
//! Each step the blade settles its style and the style button, waits out a
//! knockdown, a roll, a lock or a kick, lights or puts away, throws, reacts
//! to whatever the server said it blocked or bounced off, starts a change
//! of weapons or a kata, and then picks the next move of its chain: the
//! keys' swing (with the specials the keys and the body's state call for),
//! a creature's own choice, or the move the table chains to. Setting a move
//! picks the style's animation, the parts it plays on and its speed.
//!
//! The draws (the synced rolls a creature's choices and the chain tests
//! make) come from the arms' stream, in a fixed order the sim's bits
//! depend on.

use collision::Contents;

use super::anim::{
    Parts, is_flip, is_jump, is_kata, is_kick, is_knockdown, is_leap, is_lit_stance,
    is_slope_stance, is_special_move, is_spin, play_flags, roll_playing, single_transition,
    standing,
};
use super::weapon::{Arms, Other};
use super::{Anim, Ground, Move, events, motion_flags};
use crate::combat::pace;
use crate::combat::saber::moves::{self as table, Quad};
use crate::combat::saber::{
    Blade, Blocked, Change, Hold, Move as SaberMove, Style, Thrown, power, throw,
};
use crate::input::buttons;
use crate::math::{self, Vec3};
use rules::saber_moves::chains;
use rules::saber_moves::costs;
use rules::saber_moves::creatures;
use rules::saber_moves::specials;
use rules::saber_moves::styles;
use rules::saber_moves::timing;

use Anim::*;
use SaberMove::*;

/// Animations per style group in the animation list: a style's version of
/// a fast-style animation sits this many places on per group.
const GROUP_SIZE: i32 = 77;

/// How fast `style` plays `anim`: the fast and strong styles' transitions
/// have their own pace, everything else plays at its own.
pub(crate) fn play_speed(style: Option<Style>, anim: Anim) -> f32 {
    match style {
        Some(Style::Fast) if single_transition(anim) => styles::FAST_TRANSITIONS,
        Some(Style::Strong) if single_transition(anim) => styles::STRONG_TRANSITIONS,
        _ => 1.0,
    }
}

/// The style's animation of a move: the table's, moved to the style's
/// group where the style has its own.
fn style_anim(style: Option<Style>, next: SaberMove) -> Anim {
    let base = next.data().anim;
    let Some(style) = style else {
        return base;
    };
    let in_group = || base.offset(i32::from(style.group()) * GROUP_SIZE);
    let two_blades = matches!(style, Style::Dual | Style::Staff);
    match next {
        DRAW => match style {
            Style::Staff => BOTH_S1_S7,
            Style::Dual => BOTH_S1_S6,
            _ => base,
        },
        PUTAWAY => match style {
            Style::Staff => BOTH_S7_S1,
            Style::Dual => BOTH_S6_S1,
            _ => base,
        },
        // The guards' moves (broken, knockaways, parries and all reflects
        // but the last: a kept quirk, COMMON.md) have their own block for
        // the pair and the staff.
        _ if two_blades && next.between(V1_BR, REFLECT_LR) => {
            base.offset(if style == Style::Dual { 25 } else { 50 })
        }
        _ if two_blades && next.between(S_TL2BR, D1_B_) => in_group(),
        _ if style != Style::Fast
            && !(next.idle()
                || next.parry()
                || next.knockaway()
                || next.broken()
                || next.reflect()
                || next.special()) =>
        {
            in_group()
        }
        _ => base,
    }
}

trait Between {
    fn between(self, first: SaberMove, last: SaberMove) -> bool;
}

impl Between for SaberMove {
    fn between(self, first: SaberMove, last: SaberMove) -> bool {
        (first.index()..=last.index()).contains(&self.index())
    }
}

/// One of the eight deflections.
fn deflection(m: SaberMove) -> bool {
    m.between(D1_BR, D1_B_)
}

/// A kick's version in the air.
fn in_the_air(kick: SaberMove) -> SaberMove {
    match kick {
        KICK_F => KICK_F_AIR,
        KICK_B => KICK_B_AIR,
        KICK_R => KICK_R_AIR,
        KICK_L => KICK_L_AIR,
        other => other,
    }
}

/// The moves that play on the whole body: every kick and every special
/// but the arials, the butterflies, the spins, the leap, the mounted
/// attacks and the pair's mirror attacks.
fn whole_body(m: SaberMove) -> bool {
    m.kick()
        || (m.special()
            && !matches!(
                m,
                JUMPATTACK_ARIAL_LEFT
                    | JUMPATTACK_ARIAL_RIGHT
                    | BUTTERFLY_LEFT
                    | BUTTERFLY_RIGHT
                    | SPINATTACK
                    | SPINATTACK_DUAL
                    | SPINATTACK_ALORA
                    | LEAP_ATTACK
                    | SWOOP_ATTACK_LEFT
                    | SWOOP_ATTACK_RIGHT
                    | TAUNTAUN_ATTACK_LEFT
                    | TAUNTAUN_ATTACK_RIGHT
                    | DUAL_FB
                    | DUAL_LR
            ))
}

/// What a creature swings next from where its last move ended (none from
/// the bottom): mostly across or on round the ring, sometimes back.
const FROM_QUADRANT: [(Quad, [SaberMove; 3]); 6] = [
    (Quad::BR, [A_BR2TL, T1_BR_TR, A_R2L]),
    (Quad::R, [A_R2L, T1__R_T_, A_L2R]),
    (Quad::TR, [A_R2L, A_TR2BL, T1_TR_BR]),
    // The left side mirrors the right.
    (Quad::BL, [A_BL2TR, T1_BL_TL, A_L2R]),
    (Quad::L, [A_L2R, T1__L_T_, A_R2L]),
    (Quad::TL, [A_L2R, A_TL2BR, T1_TL_BL]),
];

/// Which way from the body a check for someone looks.
#[derive(Clone, Copy)]
enum Way {
    Front,
    Back,
    Right,
    Left,
}

impl Move<'_, '_> {
    // --- Small helpers ------------------------------------------------------

    fn attack_held(&self) -> bool {
        self.cmd.buttons & buttons::ATTACK != 0
    }

    fn alternate_held(&self) -> bool {
        self.cmd.buttons & buttons::ALT_ATTACK != 0
    }

    fn either_held(&self) -> bool {
        self.attack_held() || self.alternate_held()
    }

    fn jumping(&self) -> bool {
        self.cmd.up > 0 || self.ps.flags & motion_flags::JUMP_PRESSED != 0
    }

    fn on_ground(&self) -> bool {
        self.ps.ground.is_some()
    }

    /// The view's yaw alone: forward, right and up.
    fn flat_view(&self) -> (Vec3, Vec3, Vec3) {
        math::angle_vectors([0.0, self.ps.view[1], 0.0])
    }

    /// One synced roll in `low..=high` from the arms' stream.
    fn roll(arms: &mut Arms, low: i32, high: i32) -> i32 {
        let u = arms.rng.next_u32();
        let span = i64::from((high - low).max(0)) + 1;
        low + ((i64::from(u >> 16) * span) >> 16) as i32
    }

    /// Whether the pool holds `percent` of itself; when not, the
    /// no-power event.
    fn affords(&mut self, arms: &Arms, percent: u16) -> bool {
        let enough = arms
            .kit
            .cost(percent)
            .is_some_and(|cost| arms.fighter.fp >= cost);
        if !enough {
            self.ps.events |= events::NO_POWER;
        }
        enough
    }

    fn pay(arms: &mut Arms, percent: u16) {
        if let Some(cost) = arms.kit.cost(percent) {
            arms.fighter.fp = (arms.fighter.fp - cost).max(0);
        }
    }

    /// Where the keys put the blade: opposite a side key, raised with
    /// forward or lowered with back; the top for forward or back alone.
    fn keys_quadrant(&self) -> Quad {
        let (forward, right) = (self.cmd.forward, self.cmd.right);
        match (right.signum(), forward.signum()) {
            (1, 1) => Quad::TL,
            (1, -1) => Quad::BL,
            (1, _) => Quad::L,
            (-1, 1) => Quad::TR,
            (-1, -1) => Quad::BR,
            (-1, _) => Quad::R,
            (_, 0) => Quad::R,
            _ => Quad::T,
        }
    }

    /// The kick the keys ask for; taking it clears the key it used.
    fn keys_kick(&mut self) -> Option<SaberMove> {
        if self.cmd.right != 0 {
            let kick = if self.cmd.right > 0 { KICK_R } else { KICK_L };
            self.cmd.right = 0;
            Some(kick)
        } else if self.cmd.forward != 0 {
            let kick = if self.cmd.forward > 0 { KICK_F } else { KICK_B };
            self.cmd.forward = 0;
            Some(kick)
        } else {
            None
        }
    }

    /// How far below the box the ground is.
    fn ground_below(&self) -> f32 {
        let o = self.ps.origin;
        let below = [o[0], o[1], o[2] - 4096.0];
        let hit = self.trace_masked(o, below, self.mins, self.maxs, Contents::SOLID);
        math::length(math::sub(o, hit.end))
    }

    /// Someone within `reach` that way, met by a box of half-size `half`.
    fn someone(&self, arms: &Arms, way: Way, reach: f32, half: Vec3) -> Option<Other> {
        let (forward, right, _) = self.flat_view();
        let dir = match way {
            Way::Front => forward,
            Way::Back => math::scale(forward, -1.0),
            Way::Right => right,
            Way::Left => math::scale(right, -1.0),
        };
        let o = self.ps.origin;
        let hit = self.trace(o, math::mad(o, reach, dir), math::scale(half, -1.0), half);
        if hit.fraction >= 1.0 {
            return None;
        }
        let Ground::Entity(entity) = hit.ground else {
            return None;
        };
        arms.others
            .binary_search_by_key(&entity, |other| other.entity)
            .ok()
            .map(|i| arms.others[i])
    }

    /// The body leaps along the flat view at `forward` with `up` (units/s).
    fn leap(&mut self, forward: f32, up: f32) {
        let (ahead, _, _) = self.flat_view();
        self.ps.velocity = math::scale(ahead, forward);
        self.ps.velocity[2] = up;
        self.set_jump_start(self.ps.origin[2]);
        self.ps.events |= events::JUMP;
        self.cmd.up = 0;
    }

    // --- The step ---------------------------------------------------------

    /// One step of the blade in hand. The phases are a priority order:
    /// an earlier situation pre-empts the later ones.
    pub(super) fn saber_step(&mut self, arms: &mut Arms, blade: Blade) {
        let hand = arms.fighter.hand;
        let forced = arms.kit.forced_style(hand);
        let pair = forced == Some(Style::Dual);

        self.fit_style(arms, forced);
        self.style_button(arms, forced);

        // Down or rolling: the timer runs, and the forward roll may stab.
        let legs = self.ps.legs;
        if self.floor_anim_held() || roll_playing(self.ps, legs.anim) {
            let timer = &mut arms.fighter.weapon_wait_ms;
            if *timer > 0 {
                *timer = (*timer - self.ms).max(0);
            }
            if legs.anim == BOTH_ROLL_F
                && legs.timer <= timing::ROLL_STAB_MS
                && self.attack_held()
                && self.affords(arms, costs::FORWARD_BACK)
            {
                self.light(arms);
                self.begin_saber_move(arms, ROLL_STAB);
                Self::pay(arms, costs::FORWARD_BACK);
            }
            return;
        }

        if arms.fighter.saber.lock_until > self.time_ms {
            arms.fighter.saber.current_move = NONE;
            return;
        }
        if power::final_break(self.ps.torso.anim) && self.ps.torso.timer > 0 {
            return;
        }
        if is_kick(self.ps.legs.anim) || is_kick(self.ps.torso.anim) {
            if self.ps.legs.timer > 0 {
                return;
            }
            arms.fighter.saber.current_move = READY;
            arms.fighter.weapon_wait_ms = 0;
        }

        let unlit = !arms.fighter.saber.on;
        if unlit {
            self.unlit_step(arms);
        } else if self.lit_step(arms, blade, pair) {
            return;
        }

        if arms.fighter.weapon_wait_ms <= 0 && self.ps.torso.timer <= 0 {
            self.lower_weapon(arms);
        }
        if self.kata_possible(arms) {
            let kata = match arms.fighter.saber.style {
                Some(Style::Fast) => A1_SPECIAL,
                Some(Style::Strong) => A3_SPECIAL,
                Some(Style::Dual) => DUAL_SPIN_PROTECT,
                Some(Style::Staff) => STAFF_SOULCAL,
                Some(Style::Medium) | None => A2_SPECIAL,
            };
            self.begin_saber_move(arms, kata);
            Self::pay(arms, costs::KATA);
            return;
        }
        if arms.fighter.weapon_wait_ms > 0 {
            return;
        }

        match arms.fighter.saber.change {
            Change::Dropping => {
                self.raise_weapon(arms);
                return;
            }
            Change::Raising => {
                arms.fighter.saber.change = Change::None;
                let legs = self.ps.legs.anim;
                let anim = if forward_gait(legs) {
                    legs
                } else {
                    Self::blade_stance(&arms.fighter.saber, pair)
                };
                self.animator
                    .play(self.ps, Parts::UPPER, anim, play_flags::WHEN_FREE);
            }
            Change::None => {}
        }
        if unlit {
            return;
        }

        if arms.fighter.saber.style == Some(Style::Staff)
            && self.alternate_held()
            && let Some(kick) = self.staff_kick(arms)
        {
            self.begin_saber_move(arms, kick);
            return;
        }
        // Alternate never swings.
        self.cmd.buttons &= !buttons::ALT_ATTACK;

        if self.next_move(arms, blade) {
            let torso = self.ps.torso.timer;
            arms.fighter.weapon_wait_ms = if torso == 0 {
                timing::FIRE_TIME_MS
            } else {
                torso
            };
        }
    }

    /// The style must suit the hands: a staff with its second blade off
    /// and a single blade fight a single style, a staff or a pair with
    /// both blades on their own. Anything else takes the hand's style
    /// afresh.
    fn fit_style(&mut self, arms: &mut Arms, forced: Option<Style>) {
        let saber = &mut arms.fighter.saber;
        let single = matches!(
            saber.style,
            Some(Style::Fast | Style::Medium | Style::Strong)
        );
        let fits = match (forced, saber.second_off) {
            (Some(Style::Staff), true) => single,
            (Some(style), false) => saber.style == Some(style),
            (None, false) => single,
            _ => false,
        };
        if !fits {
            saber.style = arms.kit.style_for(arms.fighter.hand, saber.style);
            saber.second_off = false;
            saber.queued = None;
        }
    }

    /// The style button, on a press: a staff switches its second blade, a
    /// single blade cycles its styles (queued while the weapon timer runs);
    /// a pair always fights as one.
    fn style_button(&mut self, arms: &mut Arms, forced: Option<Style>) {
        let hand = arms.fighter.hand;
        let ready = arms.fighter.weapon_wait_ms <= 0;
        let held = self.cmd.buttons & buttons::SABER_STYLE != 0;
        let kit = arms.kit;
        let saber = &mut arms.fighter.saber;
        if held && !saber.cycle_held {
            match forced {
                Some(Style::Dual) => {}
                Some(Style::Staff) => {
                    saber.second_off = !saber.second_off;
                    saber.style = if saber.second_off {
                        kit.single_for(hand, None)
                    } else {
                        Some(Style::Staff)
                    };
                    saber.queued = None;
                }
                _ => {
                    let next = saber
                        .queued
                        .or(saber.style)
                        .and_then(|from| kit.next_single(hand, from));
                    if ready {
                        if next.is_some() {
                            saber.style = next;
                        }
                    } else {
                        saber.queued = next;
                    }
                }
            }
        }
        saber.cycle_held = held;
        if ready && let Some(queued) = saber.queued.take() {
            saber.style = Some(queued);
        }
    }

    /// The unlit blade: ready (unless a put-away still plays), the torso
    /// with the legs, and an attack lights it once the timer is out.
    fn unlit_step(&mut self, arms: &mut Arms) {
        let putting_away = arms.fighter.saber.current_move == PUTAWAY && self.ps.torso.timer > 0;
        if !(arms.fighter.saber.current_move == READY || putting_away) {
            self.begin_saber_move(arms, READY);
        }
        if self.ps.torso.timer <= 0 {
            self.torso_follows_legs();
        }
        let wants = self.either_held();
        if putting_away && wants {
            arms.fighter.weapon_wait_ms = 0;
            self.ps.torso.timer = 0;
        }
        if arms.fighter.weapon_wait_ms < 1 && wants {
            self.light(arms);
        }
        if arms.fighter.weapon_wait_ms > 0 {
            arms.fighter.weapon_wait_ms -= self.ms;
        }
    }

    /// The lit blade's checks before anything new: out of the hand, the
    /// throw, in the air, the staff's kick out of a return, the timer, and
    /// a block to react to. Whether the step ends here.
    fn lit_step(&mut self, arms: &mut Arms, blade: Blade, pair: bool) -> bool {
        let saber = arms.fighter.saber;
        if saber.thrown.is_some_and(|thrown| thrown.down()) {
            // A pair fights on with its other saber.
            if !pair {
                self.cmd.buttons &= !buttons::ATTACK;
            }
            self.cmd.buttons &= !buttons::ALT_ATTACK;
        }
        if self.throw(arms) {
            return true;
        }
        if arms.fighter.saber.flying() {
            let fighting_on = pair
                && !saber.second_off
                && (self.attack_held()
                    || !matches!(
                        self.ps.torso.anim,
                        BOTH_SABERDUAL_STANCE | BOTH_SABERPULL | BOTH_STAND1
                    ));
            if !fighting_on {
                self.animator.play(
                    self.ps,
                    Parts::UPPER,
                    BOTH_SABERPULL,
                    play_flags::OVER_ANY | play_flags::HOLD_FULL,
                );
                self.ps.torso.timer = 1;
                return true;
            }
        }

        let kick_from_return = self.alternate_held()
            && saber.style == Some(Style::Staff)
            && arms.fighter.weapon_wait_ms > 0
            && saber.current_move.ret()
            && saber.blocked == Blocked::None
            && !self.attack_held()
            && (self.cmd.forward != 0 || self.cmd.right != 0)
            && self.kick_allowed(arms);
        if kick_from_return && let Some(kick) = self.keys_kick() {
            arms.fighter.weapon_wait_ms = 0;
            self.begin_saber_move(arms, kick);
            return true;
        }

        if arms.fighter.weapon_wait_ms > 0 {
            arms.fighter.weapon_wait_ms -= self.ms;
        }
        if saber.blocked != Blocked::None {
            self.react(arms, blade);
            return true;
        }
        false
    }

    /// Lights the blade; from ready, nothing or a put-away it is drawn.
    pub(super) fn light(&mut self, arms: &mut Arms) {
        if arms.fighter.saber.on {
            return;
        }
        arms.fighter.saber.on = true;
        self.ps.events |= events::SABER_ON;
        if matches!(arms.fighter.saber.current_move, READY | NONE | PUTAWAY) {
            self.begin_saber_move(arms, DRAW);
            if arms.fighter.saber.current_move == DRAW {
                arms.fighter.weapon_wait_ms = arms.fighter.weapon_wait_ms.max(self.ps.torso.timer);
            }
        }
    }

    /// Throws the saber when alternate asks, the kit knows how, the Force
    /// can pay and there is room ahead. Whether it threw.
    fn throw(&mut self, arms: &mut Arms) -> bool {
        let Some(known) = arms.kit.throw else {
            return false;
        };
        let fighter = &*arms.fighter;
        let can = self.alternate_held()
            && fighter.saber.style != Some(Style::Staff)
            && fighter.weapon_wait_ms < 1
            && fighter.saber.thrown.is_none()
            && fighter.force.gripped_by.is_none()
            && !fighter.force.stunned(self.time_ms)
            && fighter.fp >= known.cost;
        if !can {
            return false;
        }
        let (ahead, _, _) = math::angle_vectors(self.ps.view);
        let o = self.ps.origin;
        let half = [throw::HALF_SIZE; 3];
        let hit = self.trace(
            o,
            math::mad(o, throw::CLEAR_AHEAD, ahead),
            math::scale(half, -1.0),
            half,
        );
        if hit.fraction < 1.0 || hit.all_solid {
            return false;
        }
        arms.fighter.fp -= known.cost;
        arms.fighter.saber.thrown = Some(Thrown::leaving(known.level, self.time_ms));
        true
    }

    /// Whether the staff may kick now: alternate held, not mid-flip (or
    /// near its end), the staff style, lit.
    fn kick_allowed(&self, arms: &Arms) -> bool {
        let legs = self.ps.legs;
        self.alternate_held()
            && (!is_flip(legs.anim) || legs.timer <= timing::FLIP_KICK_MS)
            && arms.fighter.saber.style == Some(Style::Staff)
            && arms.fighter.saber.on
    }

    /// The staff's kick from ready: the keys' kick on the ground; in the
    /// air its air version when high enough, or the ground kick when
    /// about to land.
    fn staff_kick(&mut self, arms: &Arms) -> Option<SaberMove> {
        let legs = self.ps.legs;
        if is_kick(self.ps.torso.anim)
            || is_kick(legs.anim)
            || roll_playing(self.ps, legs.anim)
            || arms.fighter.saber.current_move != READY
            || self.ps.ducked()
            || self.cmd.up < 0
        {
            return None;
        }
        let kick = self.keys_kick()?;
        if self.on_ground() {
            return Some(kick);
        }
        let above = self.ground_below();
        let vz = self.ps.velocity[2];
        if (!is_flip(legs.anim) || legs.timer <= 0)
            && above > specials::AIR_KICK_ABOVE
            && above > -vz - specials::AIR_KICK_ABOVE
        {
            Some(in_the_air(kick))
        } else if above > specials::GROUND_KICK_ABOVE || vz >= 0.0 {
            None
        } else {
            Some(kick)
        }
    }

    /// Whether the style's kata can start: standing ready (or starting a
    /// swing) on the ground with both attacks held and no keys, and the
    /// Force to pay for it.
    fn kata_possible(&mut self, arms: &Arms) -> bool {
        let saber = &arms.fighter.saber;
        let Some(style) = saber.style else {
            return false;
        };
        let current = saber.current_move;
        !arms.kit.secondary(arms.fighter.hand, style)
            && (current == READY || current.start())
            && !current.kata()
            && !is_kata(self.ps.legs.anim)
            && !is_kata(self.ps.torso.anim)
            && self.on_ground()
            && self.attack_held()
            && self.alternate_held()
            && self.cmd.forward == 0
            && self.cmd.right == 0
            && self.cmd.up <= 0
            && self.affords(arms, costs::KATA)
    }

    // --- Blocks -----------------------------------------------------------

    /// The blade reacts to what the server said it blocked or bounced off.
    fn react(&mut self, arms: &mut Arms, blade: Blade) {
        let current = arms.fighter.saber.current_move;
        let blocked = arms.fighter.saber.blocked;
        let defence = usize::from(arms.kit.defence(blade).min(3));
        let parry_hold = timing::PARRY_HOLD_MS[defence] + timing::PARRY_DEBOUNCE_MS;
        let melee = blocked.melee_parry();
        if melee {
            arms.fighter.weapon_wait_ms = parry_hold;
        }

        let guard = |m: SaberMove| Some((m, false));
        let reaction = match blocked {
            Blocked::None => None,
            Blocked::BounceMove => {
                self.ps.torso.timer = 0;
                Some((current, true))
            }
            Blocked::ParryBroken => {
                let broken = if current.broken() {
                    Some(current)
                } else {
                    table::broken_parry_for(current)
                };
                broken.map(|m| (m, true))
            }
            Blocked::AttackBounce => (current.index() < T1_BR__R.index())
                .then(|| (self.attack_bounce(arms, current), true)),
            Blocked::UpperRight => guard(PARRY_UR),
            Blocked::UpperLeft => guard(PARRY_UL),
            Blocked::LowerRight => guard(PARRY_LR),
            Blocked::LowerLeft => guard(PARRY_LL),
            Blocked::Top => guard(PARRY_UP),
            Blocked::UpperRightShot => guard(REFLECT_UR),
            Blocked::UpperLeftShot => guard(REFLECT_UL),
            Blocked::LowerRightShot => guard(REFLECT_LR),
            Blocked::LowerLeftShot => guard(REFLECT_LL),
            Blocked::TopShot => guard(REFLECT_UP),
        };
        if let Some((m, timed)) = reaction {
            self.begin_saber_move(arms, m);
            if timed {
                arms.fighter.weapon_wait_ms = self.ps.torso.timer;
            }
        }
        if melee {
            self.ps.torso.timer = self.ps.torso.timer.max(arms.fighter.weapon_wait_ms);
        }
        // A weapon change under way goes on (QUIRKS.md Q-0040).
        arms.fighter.saber.blocked = Blocked::None;
    }

    /// Where a swing bounced off something goes: an attack's bounce;
    /// otherwise back down, or (attack held) on toward the keys' quadrant,
    /// re-rolled while that is where it starts (never the bottom: QUIRKS.md
    /// Q-0037), back down when no transition goes there (Q-0039).
    fn attack_bounce(&mut self, arms: &mut Arms, current: SaberMove) -> SaberMove {
        if current.attack() && !current.bounce() {
            return table::bounce_for(current);
        }
        let start = current.data().start;
        if !self.attack_held() {
            return table::return_from(start);
        }
        let mut q = self.keys_quadrant();
        while q == start {
            q = Quad::ALL[Self::roll(arms, 0, 6) as usize];
        }
        table::transition(start, q).unwrap_or_else(|| table::return_from(start))
    }

    // --- The next move ----------------------------------------------------

    /// Picks and sets the next move. Whether a move counts as set.
    fn next_move(&mut self, arms: &mut Arms, blade: Blade) -> bool {
        let saber_move = arms.fighter.saber.current_move;
        let current = if saber_move == NONE {
            READY
        } else {
            saber_move
        };
        let row = current.data();
        let mut both_legs = false;

        let next = 'choice: {
            // The strong jump attack comes down first.
            if current == A_JUMP_T__B_ || self.ps.torso.anim == BOTH_FORCELEAP2_T__B_ {
                break 'choice R_T2B;
            }
            if !self.attack_held() {
                arms.fighter.weapon_wait_ms = 0;
                let ended = if current.start() {
                    current.attack_of_start()
                } else if current.pure_attack() {
                    current.return_of_attack()
                } else if current.transition() {
                    row.chain_attack
                } else if current.bounce() {
                    row.chain_idle
                } else {
                    self.begin_saber_move(arms, READY);
                    return false;
                };
                if ended != NONE {
                    break 'choice ended;
                }
            }
            if current.parry() || current.reflect() {
                let follow = if arms.player {
                    table::attack_from_parry(row.end)
                } else {
                    self.creature_attack_from(arms, row.end)
                };
                if let Some(follow) = follow.filter(|&m| m != NONE) {
                    break 'choice follow;
                }
            }
            let chained = self.chaining(arms, blade, current);
            if chained != NONE {
                let way = self.way_from(arms, current, chained);
                if way != NONE {
                    break 'choice way;
                }
            }
            both_legs = self.cmd.forward == 0
                && self.cmd.right == 0
                && self.cmd.up >= 0
                && self.on_ground();
            row.chain_attack
        };

        self.begin_saber_move(arms, next);
        let table_anim = next.data().anim;
        if both_legs && self.ps.torso.anim == table_anim {
            // Only the table's animation, not the style's (a kept quirk,
            // COMMON.md).
            self.animator.play(
                self.ps,
                Parts::LOWER,
                table_anim,
                play_flags::OVER_ANY | play_flags::HOLD_FULL,
            );
        }
        true
    }

    /// The chaining choice: a transition or start goes on, a broken move
    /// readies, anything else takes the wanted swing (a creature's own, or
    /// the keys'), or rests once the chain is done.
    fn chaining(&mut self, arms: &mut Arms, blade: Blade, current: SaberMove) -> SaberMove {
        let row = current.data();
        if current.transition() {
            return row.chain_attack;
        }
        if current.start() {
            return current.attack_of_start();
        }
        if current.broken() {
            return READY;
        }
        let wanted = if !arms.player && self.creature_varies(arms, blade) {
            self.creature_attack_from(arms, row.end).unwrap_or(NONE)
        } else {
            let keys = self.keys_attack(arms, current);
            if (current.bounce() || current.broken()) && keys.data().start == row.end {
                row.chain_attack
            } else {
                keys
            }
        };
        if self.chain_done(arms, current, wanted) {
            row.chain_idle
        } else {
            wanted
        }
    }

    /// How the blade gets from the current move to `next`: a start before
    /// a swing from ready, a return before ready, and a transition between
    /// swings that do not meet.
    fn way_from(&mut self, arms: &mut Arms, current: SaberMove, next: SaberMove) -> SaberMove {
        let across = || table::transition(current.data().end, next.data().start).unwrap_or(next);
        if current == READY {
            return if next.pure_attack() {
                next.start_of_attack()
            } else {
                next
            };
        }
        if next == READY {
            // From a plain attack, its own return (QUIRKS.md Q-0035).
            return if current.pure_attack() {
                current.return_of_attack()
            } else {
                READY
            };
        }
        if !next.pure_attack() {
            return next;
        }
        if next == current {
            return if self.chain_done(arms, current, next) {
                current.return_of_attack()
            } else {
                across()
            };
        }
        if current.data().end == next.data().start {
            return next;
        }
        let swung = current.pure_attack()
            || deflection(current)
            || current.ret()
            || current.parry()
            || current.reflect()
            || current.knockaway()
            || current.broken();
        if swung { across() } else { next }
    }

    /// The swing the keys ask for, with the specials they call up, then the
    /// pair's mirror attacks.
    fn keys_attack(&mut self, arms: &mut Arms, current: SaberMove) -> SaberMove {
        let Some(style) = arms.fighter.saber.style else {
            return NONE;
        };
        let hand = arms.fighter.hand;
        let secondary = arms.kit.secondary(hand, style);
        let torso = self.ps.torso.anim;

        let mut next = if self.cmd.right != 0 {
            let right = self.cmd.right > 0;
            if let Some(flip) = self.cartwheel(arms, style, secondary, right) {
                flip
            } else {
                match (right, self.cmd.forward.signum()) {
                    (true, 1) => A_TL2BR,
                    (true, -1) => A_BL2TR,
                    (true, _) => A_L2R,
                    (false, 1) => A_TR2BL,
                    (false, -1) => A_BR2TL,
                    (false, _) => A_R2L,
                }
            }
        } else if self.cmd.forward > 0 {
            self.forward_attack(arms, style, secondary)
        } else if self.cmd.forward < 0 {
            self.back_attack(arms, style, secondary, torso)
        } else if current.bounce() || (!arms.player && current.knockaway()) {
            let varies = !arms.player && Self::roll(arms, 0, creatures::BOUNCE_ONE_IN - 1) != 0;
            let m = if varies {
                self.creature_attack_from(arms, current.data().end)
                    .unwrap_or(NONE)
            } else {
                current.data().chain_attack
            };
            if self.chain_done(arms, current, m) {
                current.data().chain_idle
            } else {
                m
            }
        } else if current == READY {
            match arms.enemy {
                Some(enemy) if !arms.player => self.creature_at_enemy(style, enemy),
                _ => A_T2B,
            }
        } else {
            NONE
        };

        if style == Style::Dual && !is_special_move(torso) && !is_special_move(self.ps.legs.anim) {
            let reach = specials::MIRROR_REACH;
            let half = [specials::MIRROR_BOX; 3];
            let both = |step: &Self, a: Way, b: Way| {
                step.someone(arms, a, reach, half).is_some()
                    && step.someone(arms, b, reach, half).is_some()
            };
            if matches!(next, A_R2L | S_R2L | A_L2R | S_L2R) && both(self, Way::Right, Way::Left) {
                next = DUAL_LR;
                self.cmd.right = 0;
            } else if matches!(next, A_T2B | S_T2B | A_BACK | A_BACK_CR)
                && both(self, Way::Front, Way::Back)
            {
                next = DUAL_FB;
                self.cmd.forward = 0;
            }
        }
        next
    }

    /// A side key while jumping up off the ground: the staff's butterfly or
    /// anyone else's arial, flung sideways.
    fn cartwheel(
        &mut self,
        arms: &mut Arms,
        style: Style,
        secondary: bool,
        right: bool,
    ) -> Option<SaberMove> {
        let flips = !secondary
            && self.ps.velocity[2] > specials::CARTWHEEL_RISE
            && self.attack_held()
            && self.ground_below() < specials::CARTWHEEL_GROUND
            && self.jumping()
            && self.affords(arms, costs::SIDE);
        if !flips {
            return None;
        }
        Self::pay(arms, costs::SIDE);
        let (_, side, _) = self.flat_view();
        let speed = if right {
            specials::CARTWHEEL_SPEED
        } else {
            -specials::CARTWHEEL_SPEED
        };
        self.ps.velocity[0] = 0.0;
        self.ps.velocity[1] = 0.0;
        self.ps.velocity = math::mad(self.ps.velocity, speed, side);
        let lift = |pair: [f32; 2]| if right { pair[0] } else { pair[1] };
        Some(if style == Style::Staff {
            self.ps.velocity[2] = lift(specials::BUTTERFLY_LIFT);
            if right {
                BUTTERFLY_RIGHT
            } else {
                BUTTERFLY_LEFT
            }
        } else {
            self.ps.events |= events::JUMP;
            self.ps.velocity[2] = lift(specials::ARIAL_LIFT);
            if right {
                JUMPATTACK_ARIAL_RIGHT
            } else {
                JUMPATTACK_ARIAL_LEFT
            }
        })
    }

    /// Back held: the staff's back flip, a back attack on someone behind,
    /// or the plain downward swing.
    fn back_attack(
        &mut self,
        arms: &mut Arms,
        style: Style,
        secondary: bool,
        torso: Anim,
    ) -> SaberMove {
        let grounded = self.on_ground() || self.ground_below() <= specials::JUMP_GROUND;
        let current = arms.fighter.saber.current_move;
        let flips = !secondary
            && style == Style::Staff
            && arms.kit.force_jump >= specials::BACK_FLIP_JUMP
            && grounded
            && self.ps.velocity[2] >= 0.0
            && self.jumping()
            && !(current.in_between() || current.attack())
            && arms.fighter.weapon_wait_ms <= 0
            && self.attack_held()
            && self.affords(arms, costs::FORWARD_BACK);
        if flips {
            // Paid at the forward/back cost (QUIRKS.md Q-0041).
            Self::pay(arms, costs::FORWARD_BACK);
            self.cmd.up = 127;
            self.ps.velocity[2] = specials::BACK_FLIP_LIFT;
            return A_BACKFLIP_ATK;
        }
        let behind = self
            .someone(arms, Way::Back, specials::BACK_REACH, specials::BACK_BOX)
            .is_some();
        if behind && !is_special_move(torso) {
            return match style {
                Style::Fast | Style::Staff => A_BACKSTAB,
                _ if self.ps.ducked() || self.cmd.up < 0 => A_BACK_CR,
                _ => A_BACK,
            };
        }
        A_T2B
    }

    /// Forward held: the two-bladed jump attacks, the medium flip and the
    /// strong leap, the crouched lunge or spin, or the stab down at someone
    /// lying in front.
    fn forward_attack(&mut self, arms: &mut Arms, style: Style, secondary: bool) -> SaberMove {
        let saber = arms.fighter.saber;
        let legs = self.ps.legs.anim;
        let torso = self.ps.torso.anim;
        let vz = self.ps.velocity[2];

        let jump_attack = !secondary
            && matches!(style, Style::Dual | Style::Staff)
            && (self.on_ground() || self.ground_below() <= specials::JUMP_GROUND)
            && vz >= 0.0
            && self.jumping()
            && !(saber.current_move.in_between() || saber.current_move.attack())
            && arms.fighter.weapon_wait_ms <= 0
            && self.attack_held()
            && self.affords(arms, costs::FORWARD_BACK);
        if jump_attack {
            Self::pay(arms, costs::FORWARD_BACK);
            return if style == Style::Dual {
                self.cmd.up = 0;
                JUMPATTACK_DUAL
            } else {
                JUMPATTACK_STAFF_RIGHT
            };
        }

        let leaps = |step: &mut Self, arms: &Arms| {
            step.ps.velocity[2] > specials::LEAP_RISE
                && step.ground_below() < specials::LEAP_GROUND
                && !is_leap(legs)
                && !is_special_move(torso)
                && step.affords(arms, costs::FORWARD_BACK)
        };
        if !secondary && style == Style::Medium && leaps(self, arms) {
            Self::pay(arms, costs::FORWARD_BACK);
            self.leap(specials::FLIP_LEAP[0], specials::FLIP_LEAP[1]);
            return A_FLIP_SLASH;
        }
        if !secondary && style == Style::Strong && leaps(self, arms) {
            Self::pay(arms, costs::FORWARD_BACK);
            self.leap(specials::STRONG_LEAP[0], specials::STRONG_LEAP[1]);
            return A_JUMP_T__B_;
        }

        let crouched = matches!(style, Style::Fast | Style::Dual | Style::Staff)
            && self.on_ground()
            && self.ps.ducked()
            && arms.fighter.weapon_wait_ms <= 0
            && !is_special_move(torso)
            && self.affords(arms, costs::FORWARD_BACK);
        if crouched {
            let m = match style {
                Style::Fast => {
                    let (ahead, _, _) = self.flat_view();
                    self.ps.velocity = math::scale(ahead, specials::LUNGE_SPEED);
                    self.ps.events |= events::JUMP;
                    A_LUNGE
                }
                Style::Staff => SPINATTACK,
                _ => SPINATTACK_DUAL,
            };
            Self::pay(arms, costs::FORWARD_BACK);
            return m;
        }

        if secondary {
            return NONE;
        }
        if !self.on_ground() {
            return A_T2B;
        }
        if arms.player {
            self.ps.velocity[2] = 0.0;
            self.cmd.up = 0;
        }
        let lying = self
            .someone(
                arms,
                Way::Front,
                specials::STAB_REACH,
                [specials::STAB_BOX; 3],
            )
            .is_some_and(|other| is_knockdown(other.legs));
        if lying && self.affords(arms, costs::FORWARD_BACK) {
            Self::pay(arms, costs::FORWARD_BACK);
            return match style {
                Style::Dual => STABDOWN_DUAL,
                Style::Staff => STABDOWN_STAFF,
                _ => STABDOWN,
            };
        }
        A_T2B
    }

    // --- Creatures --------------------------------------------------------

    /// A creature's swing from quadrant `q`: from the top a coin between
    /// two downward swings; from the bottom none; elsewhere one in a few
    /// straight across, else a coin between the other two.
    fn creature_attack_from(&mut self, arms: &mut Arms, q: Quad) -> Option<SaberMove> {
        match q {
            Quad::T => Some(if Self::roll(arms, 0, 1) != 0 {
                A_T2B
            } else {
                A_TR2BL
            }),
            Quad::B => None,
            _ => {
                let (_, [first, second, third]) =
                    *FROM_QUADRANT.iter().find(|(from, _)| *from == q)?;
                Some(if Self::roll(arms, 0, creatures::ACROSS_ONE_IN - 1) == 0 {
                    first
                } else if Self::roll(arms, 0, 1) == 0 {
                    second
                } else {
                    third
                })
            }
        }
    }

    /// Whether a creature's next swing in a chain comes from where its last
    /// ended rather than its keys: likelier the better its offence.
    fn creature_varies(&mut self, arms: &mut Arms, blade: Blade) -> bool {
        let style = arms.fighter.saber.style;
        let offence = style.map_or(1, |style| i32::from(arms.kit.level(style, blade).max(1)));
        if Self::roll(arms, 0, offence - 1) != 0 {
            return true;
        }
        style == Some(Style::Fast) && pace::RANK >= pace::FENCER && Self::roll(arms, 0, 1) != 0
    }

    /// A creature's swing from ready at its enemy's eye: across or
    /// diagonally toward the side it stands on, or a back attack.
    fn creature_at_enemy(&self, style: Style, enemy: Vec3) -> SaberMove {
        let (forward, right, up) = self.flat_view();
        let d = math::sub(enemy, self.ps.origin);
        let distance = math::length(d);
        let n = math::scale(d, 1.0 / distance.max(f32::EPSILON));
        let facing = math::dot(n, forward);
        let low = self.ps.ducked() || self.cmd.up < 0;
        if facing > 0.0 {
            let side = math::dot(n, right);
            let above = math::dot(n, up) > creatures::ABOVE;
            return if side > creatures::SIDE_ACROSS {
                A_L2R
            } else if side < -creatures::SIDE_ACROSS {
                A_R2L
            } else if side > creatures::SIDE_DIAGONAL {
                if above { A_BL2TR } else { A_TR2BL }
            } else if side < -creatures::SIDE_DIAGONAL {
                if above { A_BR2TL } else { A_TL2BR }
            } else {
                A_T2B
            };
        }
        let quick = matches!(style, Style::Fast | Style::Staff);
        if !self.on_ground() {
            NONE
        } else if quick && facing < -creatures::BEHIND && distance < creatures::BACK_STAB_REACH {
            if low { NONE } else { A_BACKSTAB }
        } else if quick {
            NONE
        } else if low {
            A_BACK_CR
        } else {
            A_BACK
        }
    }

    // --- Chains -----------------------------------------------------------

    /// Whether a chain of attacks has run its course before `next`: the
    /// strong style tires soon and at sharp turns, the fast and medium
    /// sometimes, the others never.
    fn chain_done(&mut self, arms: &mut Arms, current: SaberMove, next: SaberMove) -> bool {
        let chain = i32::from(arms.fighter.saber.chain);
        match arms.fighter.saber.style {
            Some(Style::Strong) => {
                if current == NONE || next == NONE {
                    let single = chains::STRONG_SINGLE;
                    return chain > Self::roll(arms, single[0], single[1]);
                }
                let limit = chains::STRONG_LIMIT;
                if chain > Self::roll(arms, limit[0], limit[1]) {
                    return true;
                }
                if chain <= 0 {
                    return false;
                }
                let angle = table::chain_angle(current, next);
                let [low, high] = chains::STRONG_ANGLES;
                if !(low..=high).contains(&angle) {
                    true
                } else if angle == chains::STRONG_SQUARE {
                    chain > chains::STRONG_SQUARE_CHAIN
                } else {
                    chain > chains::STRONG_OTHER_CHAIN
                }
            }
            Some(style @ (Style::Fast | Style::Medium)) => {
                let tolerance = if style == Style::Fast {
                    chains::TOLERANCE[0]
                } else {
                    chains::TOLERANCE[1]
                };
                if next.pure_attack()
                    && next != A_T2B
                    && chain >= tolerance
                    && Self::roll(arms, 1, chain) > tolerance
                {
                    return true;
                }
                if style == Style::Medium {
                    let limit = chains::MEDIUM_LIMIT;
                    return chain > Self::roll(arms, limit[0], limit[1]);
                }
                false
            }
            _ => false,
        }
    }

    // --- Setting a move ---------------------------------------------------

    /// Makes `next` the current move: its style's animation on the parts
    /// it plays on, the chain counted, the attack event; it becomes
    /// current only when the torso took the animation.
    pub(super) fn begin_saber_move(&mut self, arms: &mut Arms, next: SaberMove) {
        let style = arms.fighter.saber.style;
        let row = next.data();
        let current = arms.fighter.saber.current_move;

        let mut set = match row.hold {
            Hold::Active => play_flags::OVER_ANY | play_flags::HOLD_FULL | play_flags::HOLD_MOVE,
            Hold::Finish => play_flags::HOLD_FULL,
            Hold::Idle => play_flags::WHEN_FREE,
        };
        let saber = &mut arms.fighter.saber;
        if matches!(next, READY | A_FLIP_STAB | A_FLIP_SLASH) {
            saber.chain = 0;
        } else if next.attack() {
            saber.chain = (saber.chain + 1).min(timing::CHAIN_CAP);
        }

        let mut anim = style_anim(style, next);
        // Only the current move's table animation is compared (a kept
        // quirk, COMMON.md).
        if current.data().anim == anim && next.index() > PUTAWAY.index() {
            set |= play_flags::ANEW;
        }
        if next.special() {
            set |= play_flags::OVER_ANY;
        }

        let pair = arms.kit.forced_style(arms.fighter.hand) == Some(Style::Dual);
        let stance = Self::blade_stance(&arms.fighter.saber, pair);
        let legs = self.ps.legs.anim;
        if is_lit_stance(anim) || anim == BOTH_STAND1 {
            let still_legs = standing(legs)
                || self.ps.ducked()
                || matches!(legs, BOTH_WALK1 | BOTH_WALKBACK1 | BOTH_WALKBACK2)
                || is_slope_stance(legs);
            anim = if still_legs { stance } else { legs };
        }

        let parts = if matches!(next, JUMPATTACK_ARIAL_LEFT | JUMPATTACK_ARIAL_RIGHT) {
            Parts::LOWER
        } else if whole_body(next) || is_spin(anim) {
            Parts::WHOLE
        } else if self.cmd.forward == 0 && self.cmd.right == 0 && self.cmd.up == 0 {
            let planted = !is_flip(legs)
                && !roll_playing(self.ps, legs)
                && !self.floor_anim_held()
                && !is_jump(legs)
                && !is_leap(legs)
                && anim != stance
                && self.on_ground();
            if !self.ps.ducked() && (matches!(next, SPINATTACK | SPINATTACK_DUAL) || planted) {
                Parts::WHOLE
            } else {
                Parts::UPPER
            }
        } else {
            Parts::UPPER
        };

        self.animator
            .play_at(self.ps, parts, anim, set, play_speed(style, anim));
        if parts != Parts::LOWER
            && matches!(self.ps.legs.anim, BOTH_ARIAL_LEFT | BOTH_ARIAL_RIGHT)
            && self.ps.legs.timer > self.ps.torso.timer
        {
            self.ps.legs.timer = self.ps.torso.timer;
        }
        // An arial, set on the legs only, takes only when the torso already
        // plays it (a kept quirk, COMMON.md).
        if self.ps.torso.anim != anim {
            return;
        }

        let directional_kick = matches!(
            next,
            KICK_F | KICK_B | KICK_R | KICK_L | KICK_F_AIR | KICK_B_AIR | KICK_R_AIR | KICK_L_AIR
        );
        if (next.attack() || is_special_move(anim)) && next != current && !directional_kick {
            self.ps.events |= events::SABER_ATTACK;
        }
        if next.special() {
            arms.fighter.weapon_wait_ms = arms.fighter.weapon_wait_ms.max(self.ps.torso.timer);
        }
        let saber = &mut arms.fighter.saber;
        saber.current_move = next;
        saber.blocking = row.block;
        if arms.fighter.weapon_wait_ms <= 0 {
            arms.fighter.saber.blocked = Blocked::None;
        }
    }
}

/// A forward walk or run (any name starting `BOTH_WALK` or `BOTH_RUN`
/// without `BACK`).
fn forward_gait(anim: Anim) -> bool {
    matches!(
        anim,
        BOTH_RUN1
            | BOTH_RUN2
            | BOTH_RUN_DUAL
            | BOTH_RUN_STAFF
            | BOTH_WALK1
            | BOTH_WALK2
            | BOTH_WALK_DUAL
            | BOTH_WALK_STAFF
    )
}
