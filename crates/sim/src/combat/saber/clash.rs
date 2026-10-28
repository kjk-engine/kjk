//! What happens when one blade (A, the swinger) meets another (D): the
//! decision tree that knocks A's attack away, breaks D's parry, bounces
//! A's swing off a strong parry, settles two attacks against each other,
//! and failing all that has D block, crushing its parry or turning A
//! aside. Also the turn of an attacking blade off another (the
//! deflection), the game's whole-number roll every saber rule rolls
//! with, and whether a beaten fighter loses the saber from its hand.

use crate::math::Vec3;
use crate::motion::{Anim, MotionState};
use crate::rng::SimRng;

use super::moves::{self, Quad};
use super::power::{self, deflecting};
use super::{Blocked, Move, SaberState, Style, defence};
use rules::saber_throw::down;
use rules::sabers::clash;

/// One side of a clash, as the tree reads and writes it.
#[derive(Debug, Clone, Copy)]
pub struct Fencer {
    pub saber: SaberState,
    pub ps: MotionState,
    pub style: Style,
    /// Saber offence (the style's level, C8) and defence (C6).
    pub offence: i32,
    pub defence: i32,
    /// Attack power by its own attacking test (4.6).
    pub power: i32,
    /// Whether it attacks (`power::attacking`).
    pub attacking: bool,
    /// Animation power (as the swinger, or as a defender) with the
    /// single-saber kata's bonus.
    pub anim_power: i32,
    pub weapon_wait_ms: i32,
}

/// A whole number from `low` to `high`, both included, as the game rolls
/// it. A range upside down by one gives `low` without rolling; one upside
/// down by more rolls the game's raw 15-bit number and gives `low` only
/// when that is zero, else `low - 1`.
pub fn roll_between(rng: &mut SimRng, low: i32, high: i32) -> i32 {
    if high >= low {
        let count = (i64::from(high) - i64::from(low) + 1) as u32;
        (i64::from(low) + i64::from(rng.below(count))) as i32
    } else if high == low - 1 || rng.below(1 << 15) == 0 {
        low
    } else {
        low - 1
    }
}

/// Heads (a roll of 0 or 1 that gives 1).
fn coin(rng: &mut SimRng) -> bool {
    roll_between(rng, 0, 1) == 1
}

/// A roll from 1 to 10.
fn d10(rng: &mut SimRng) -> i32 {
    roll_between(rng, 1, 10)
}

/// What the tree decided beyond the moves it wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Result {
    /// A's saber may be knocked away (a knockaway or a broken attack).
    pub disarm_a: bool,
    /// D's may (a broken parry).
    pub disarm_d: bool,
}

/// The broken attack at the quadrant `attack` starts from.
pub fn broken_attack(attack: Move) -> Move {
    match attack.data().start {
        Quad::BR => Move::V1_BR,
        Quad::R => Move::V1__R,
        Quad::TR => Move::V1_TR,
        Quad::T => Move::V1_T_,
        Quad::TL => Move::V1_TL,
        Quad::L => Move::V1__L,
        Quad::BL => Move::V1_BL,
        Quad::B => Move::V1_B_,
    }
}

/// The deflection that ends at a quadrant.
fn deflection_at(quad: Quad) -> Move {
    match quad {
        Quad::BR => Move::D1_BR,
        Quad::R => Move::D1__R,
        Quad::TR => Move::D1_TR,
        Quad::T => Move::D1_T_,
        Quad::TL => Move::D1_TL,
        Quad::L => Move::D1__L,
        Quad::BL => Move::D1_BL,
        Quad::B => Move::D1_B_,
    }
}

/// The guard a melee blocked state stands for.
fn guard_of_block(blocked: Blocked) -> Option<Quad> {
    match blocked {
        Blocked::UpperRight => Some(Quad::TR),
        Blocked::UpperLeft => Some(Quad::TL),
        Blocked::LowerRight => Some(Quad::BR),
        Blocked::LowerLeft => Some(Quad::BL),
        Blocked::Top => Some(Quad::T),
        _ => None,
    }
}

/// The guard a parry holds.
fn guard_of_parry(parry: Move) -> Option<Quad> {
    match parry {
        Move::PARRY_UP => Some(Quad::T),
        Move::PARRY_UR => Some(Quad::TR),
        Move::PARRY_UL => Some(Quad::TL),
        Move::PARRY_LR => Some(Quad::BR),
        Move::PARRY_LL => Some(Quad::BL),
        _ => None,
    }
}

/// The knockaway from a guard.
fn knockaway_at(guard: Quad) -> Move {
    match guard {
        Quad::TR => Move::K1_TR,
        Quad::TL => Move::K1_TL,
        Quad::BR => Move::K1_BR,
        Quad::BL => Move::K1_BL,
        _ => Move::K1_T_,
    }
}

/// The parry at a guard.
fn parry_at(guard: Quad) -> Move {
    match guard {
        Quad::TR => Move::PARRY_UR,
        Quad::TL => Move::PARRY_UL,
        Quad::BR => Move::PARRY_LR,
        Quad::BL => Move::PARRY_LL,
        _ => Move::PARRY_UP,
    }
}

/// What a parry beaten in a clash breaks into. The server's own rule,
/// not M2's `broken_parry_for`: the top parry breaks up or down on a
/// coin, and the lower two break on their own side.
fn beaten_parry(parry: Move, rng: &mut SimRng) -> Move {
    match parry {
        Move::PARRY_UP => {
            if coin(rng) {
                Move::H1_B_
            } else {
                Move::H1_T_
            }
        }
        Move::PARRY_UR => Move::H1_TR,
        Move::PARRY_UL => Move::H1_TL,
        Move::PARRY_LR => Move::H1_BR,
        Move::PARRY_LL => Move::H1_BL,
        Move::READY => Move::H1_B_,
        _ => Move::NONE,
    }
}

/// Left for right; top and bottom stay.
fn mirror(quad: Quad) -> Quad {
    match quad {
        Quad::BR => Quad::BL,
        Quad::R => Quad::L,
        Quad::TR => Quad::TL,
        Quad::TL => Quad::TR,
        Quad::L => Quad::R,
        Quad::BL => Quad::BR,
        other => other,
    }
}

/// The quadrants round the view.
fn places() -> i32 {
    Quad::ALL.len() as i32
}

/// The quadrant `place` steps clockwise from the bottom right, round the
/// view.
fn quad_at(place: i32) -> Quad {
    Quad::ALL[place.rem_euclid(places()) as usize]
}

/// The short turn from one quadrant's place to another's, -3 to 4.
fn turn(from: i32, to: i32) -> i32 {
    let steps = (to - from).rem_euclid(places());
    if steps > places() / 2 {
        steps - places()
    } else {
        steps
    }
}

/// Half a turn, rounded up.
fn half_up(steps: i32) -> i32 {
    if steps >= 0 {
        (steps + 1) / 2
    } else {
        steps / 2
    }
}

/// Whether `torso` is `base` or its counterpart in one of the other style
/// groups.
pub fn attack_anim(torso: Anim, base: Anim) -> bool {
    let first = Anim::BOTH_A1_T__B_.index();
    let group = i32::from(Anim::BOTH_A2_T__B_.index() - first);
    let groups = (i32::from(Anim::BOTH_D7_B____.index() - first) + 1) / group;
    (0..groups).any(|k| torso == base.offset(group * k))
}

/// Parrying, beaten, in a special or knocked off its line: in no state
/// to take a blow as a plain block.
fn reeling(saber_move: Move) -> bool {
    saber_move.parry() || saber_move.broken() || saber_move.special() || shaken(saber_move)
}

/// Bounced, deflected or reflecting: the contest's narrower test.
fn shaken(saber_move: Move) -> bool {
    saber_move.bounce() || deflecting(saber_move) || saber_move.reflect()
}

/// A's swing bounces off.
fn bounce_off(a: &mut Fencer) {
    a.saber.current_move = moves::bounce_for(a.saber.current_move);
    a.saber.blocked = Blocked::AttackBounce;
}

/// A's attacking blade turned by D's, with D's move as given.
fn turn_aside(a: &mut Fencer, theirs: Move, their_power: i32, rng: &mut SimRng) -> bool {
    if theirs == Move::READY {
        return false;
    }
    let mine = a.saber.current_move.data();
    let start = mine.start as i32;
    let end = mine.end as i32;
    let against = mirror(theirs.data().end) as i32;
    let apart = turn(start, against).abs();
    let square = apart == 0 || (apart == 1 && coin(rng));
    if square && (their_power == a.power || roll_between(rng, 0, their_power - a.power) >= 0) {
        bounce_off(a);
        return false;
    }
    let mut to = (end + half_up(turn(end, against))).rem_euclid(places());
    if to == start {
        to = if coin(rng) { to - 1 } else { to + 1 };
    }
    let to = quad_at(to);
    if to as i32 == against {
        bounce_off(a);
        return false;
    }
    a.saber.current_move = deflection_at(to);
    a.saber.blocked = Blocked::BounceMove;
    true
}

/// A's attacking blade turned by D's: on a square meeting that D's power
/// carries, A bounces; otherwise it is turned to the quadrant halfway
/// round to D's (never back where it started). True when it was turned.
/// D does not change.
pub fn deflect(a: &mut Fencer, d: &Fencer, rng: &mut SimRng) -> bool {
    turn_aside(a, d.saber.current_move, d.power, rng)
}

/// The tree's facts that hold for the whole call.
struct Clash {
    point: Vec3,
    met: bool,
    /// Each side's move as it came in was a special.
    a_special: bool,
    d_special: bool,
    offence_decided: bool,
    defence_decided: bool,
    result: Result,
}

impl Clash {
    /// D knocks A's attack away: by luck (more often against a weak
    /// attack) or by power, when D is steady, has power or defence
    /// enough, and A is not in a special.
    fn knocks_away(&self, a: &Fencer, d: &Fencer, rng: &mut SimRng) -> bool {
        let (am, dm) = (a.saber.current_move, d.saber.current_move);
        ((a.power < clash::STRONG && d10(rng) <= clash::KNOCK_LATE)
            || (d10(rng) <= clash::KNOCK_STRONG && d.power >= clash::STRONG))
            && !am.bounce()
            && !dm.broken()
            && !self.d_special
            && !dm.bounce()
            && !deflecting(dm)
            && !dm.reflect()
            && (d.power >= clash::STRONG
                || (d.defence >= clash::KNOCK_DEFENCE && roll_between(rng, 0, d.power) != 0))
            && !self.a_special
    }

    fn knock_away(&mut self, a: &mut Fencer, d: &mut Fencer) {
        self.result.disarm_a = true;
        let guard = if d.saber.current_move.parry() {
            guard_of_parry(d.saber.current_move)
        } else {
            d.saber.blocked = defence::parry_for(&d.ps, self.point, false);
            guard_of_block(d.saber.blocked)
        };
        d.saber.current_move = guard.map_or(Move::K1_T_, knockaway_at);
        d.saber.blocked = Blocked::BounceMove;
        a.saber.current_move = broken_attack(a.saber.current_move);
        a.saber.blocked = Blocked::BounceMove;
        self.defence_decided = true;
    }

    /// A strong attack (or a special) breaks D's parry when D's defence
    /// is lower, or equal and a roll beats D's power.
    fn breaks_parry(&self, a: &Fencer, d: &Fencer, rng: &mut SimRng) -> bool {
        let (am, dm) = (a.saber.current_move, d.saber.current_move);
        (a.power >= clash::STRONG || self.a_special)
            && (d.defence < a.power
                || (d.defence == a.power
                    && (d10(rng) as f32 >= d.power as f32 * clash::PARRY_BREAK || self.a_special)))
            && dm.parry()
            && !dm.broken()
            && !am.parry()
            && !am.broken()
            && !am.bounce()
            && !self.d_special
    }

    fn break_parry(&mut self, d: &mut Fencer, rng: &mut SimRng) {
        self.result.disarm_d = true;
        d.saber.current_move = beaten_parry(d.saber.current_move, rng);
        d.saber.blocked = Blocked::ParryBroken;
        self.defence_decided = true;
    }

    /// Two strong sides, D parrying and A swinging cleanly.
    fn strong_parry(&self, a: &Fencer, d: &Fencer) -> bool {
        let (am, dm) = (a.saber.current_move, d.saber.current_move);
        a.power >= clash::STRONG
            && d.power >= clash::STRONG
            && dm.parry()
            && !dm.broken()
            && !am.parry()
            && !am.broken()
            && !am.bounce()
            && !deflecting(am)
            && !am.reflect()
            && !self.a_special
    }

    /// Two attacks: the side with the clear advantage (animation power
    /// and offence) breaks the other's; a slight one only bounces D; a
    /// tie goes to D.
    fn contest(&mut self, a: &mut Fencer, d: &mut Fencer) {
        if shaken(a.saber.current_move) || shaken(d.saber.current_move) {
            return;
        }
        let advantage = (a.anim_power + a.offence) - (d.anim_power + d.offence);
        if advantage > clash::CONTEST_MARGIN {
            d.saber.current_move = broken_attack(d.saber.current_move);
            d.saber.blocked = Blocked::BounceMove;
        } else if advantage > 0 {
            d.saber.blocked = Blocked::AttackBounce;
        } else {
            a.saber.current_move = broken_attack(a.saber.current_move);
            a.saber.blocked = Blocked::BounceMove;
        }
        self.offence_decided = true;
    }

    /// D blocks: it parries where A came in, and either the parry is
    /// crushed, or A is turned aside, or the two attacks bounce.
    fn block(&mut self, a: &mut Fencer, d: &mut Fencer, rng: &mut SimRng) {
        let mut crush = self.a_special;
        let mut retry = false;
        let parry =
            |d: &mut Fencer, point| d.saber.blocked = defence::parry_for(&d.ps, point, false);
        if !d.attacking {
            parry(d, self.point);
            let overwhelms = a.anim_power + a.offence > power::style_level(d.style) + d.defence;
            if self.met && overwhelms {
                crush = true;
            } else {
                retry = true;
            }
        } else if a.power > d.power {
            parry(d, self.point);
            crush = true;
            self.result.disarm_d = true;
        } else if a.power == d.power {
            if d10(rng) <= clash::CRUSH_TIE {
                parry(d, self.point);
                crush = true;
                self.result.disarm_d = true;
            } else {
                if !self.offence_decided && !reeling(a.saber.current_move) && !self.a_special {
                    a.saber.blocked = Blocked::AttackBounce;
                    self.offence_decided = true;
                }
                if !self.a_special {
                    d.saber.blocked = Blocked::AttackBounce;
                }
            }
        } else if !self.a_special {
            a.saber.current_move = broken_attack(a.saber.current_move);
            a.saber.blocked = Blocked::ParryBroken;
            self.result.disarm_a = true;
            self.offence_decided = true;
        }

        let Some(pending) = guard_of_block(d.saber.blocked).map(parry_at) else {
            return;
        };
        if crush {
            d.saber.current_move = beaten_parry(pending, rng);
            d.saber.blocked = Blocked::ParryBroken;
        } else if !self.offence_decided && retry {
            turn_aside(a, pending, d.power, rng);
        }
    }
}

/// A swung at D and the blades met (`met`) or D blocked by its state, at
/// `point`. Writes both sides' moves and blocked states and says who may
/// be disarmed. The branches are tried in turn (the knockaway, the broken
/// parry, the strong parry's bounce, the contest of two attacks: the
/// first that holds alone runs), then D blocks unless that settled D's
/// part. A roll is drawn only when its test is reached.
pub fn resolve(a: &mut Fencer, d: &mut Fencer, point: Vec3, met: bool, rng: &mut SimRng) -> Result {
    let mut tree = Clash {
        point,
        met,
        a_special: a.saber.current_move.special(),
        d_special: d.saber.current_move.special(),
        offence_decided: false,
        defence_decided: false,
        result: Result::default(),
    };
    if tree.d_special {
        d.saber.blocked = Blocked::None;
    }

    if tree.knocks_away(a, d, rng) {
        tree.knock_away(a, d);
    } else if tree.breaks_parry(a, d, rng) {
        tree.break_parry(d, rng);
    } else if tree.strong_parry(a, d) {
        deflect(a, d, rng);
        tree.offence_decided = true;
    } else if d.attacking && !tree.d_special {
        tree.contest(a, d);
    }

    if !tree.defence_decided && !tree.d_special && !reeling(d.saber.current_move) {
        tree.block(a, d, rng);
    }
    tree.result
}

/// Whether the loser of a clash drops its saber: likelier the more the
/// winner's style outweighs the loser's, never when it does not.
pub fn disarm(loser: Style, winner: Style, rng: &mut SimRng) -> bool {
    let (lost, won) = (power::style_power(loser), power::style_power(winner));
    let odds = if won > lost + 1 {
        down::DISARM_ODDS[0]
    } else if won > lost {
        down::DISARM_ODDS[1]
    } else {
        return false;
    };
    d10(rng) <= odds && coin(rng)
}
