//! How much weight a blade carries into a clash.
//!
//! A style has a level (fast, medium, strong). A swing's **attack power**
//! is that level, raised while it attacks and by the hilt's travel over
//! the game's frame; the clash tree weighs one side's against the other's.
//! An animation lends **animation power** while it plays: the style
//! groups each their own, the guards theirs, and the special attacks and a
//! lock's super-break wins only inside their windows (the first and last
//! milliseconds of each are out). Also here: the two animation sets of a
//! super-break, and the single-saber katas' bonus.

use crate::motion::{Anim, AnimTable};

use super::{Blocked, Move, Style};
use Anim::*;
use rules::sabers::breaks;
use rules::sabers::power;
use rules::sabers::windows::back;
use rules::sabers::windows::down;
use rules::sabers::windows::katas;
use rules::sabers::windows::leaps;
use rules::sabers::windows::pull;
use rules::sabers::windows::spins;
use rules::sabers::windows::two_weapons;

/// The hilt's travel between JA's two last saber updates (JA's server
/// frames) the swing bonus reads; the sim's tick is shorter, so the tick's
/// travel is scaled to that frame.
pub const JA_FRAME_MS: u32 = rules::sabers::ja::FRAME_MS;

/// The style's level: fast, medium (the pair and the staff fight at it
/// too) or strong.
pub fn style_level(style: Style) -> i32 {
    match style {
        Style::Fast => power::STYLE_LEVEL[0],
        Style::Medium | Style::Dual | Style::Staff => power::STYLE_LEVEL[1],
        Style::Strong => power::STYLE_LEVEL[2],
    }
}

/// One of the eight deflections, the bottom one included (M2's `bounce`
/// leaves that one out).
pub fn deflecting(saber_move: Move) -> bool {
    (Move::D1_BR..=Move::D1_B_).contains(&saber_move)
}

/// Whether the blade attacks: never while it parries, reels, is
/// deflected, bounces or knocks away; a special always; a plain attack
/// while its weapon timer runs and nothing has blocked it.
pub fn attacking(saber_move: Move, blocked: Blocked, weapon_wait_ms: i32) -> bool {
    let reeling = saber_move.parry()
        || saber_move.broken()
        || deflecting(saber_move)
        || saber_move.bounce()
        || saber_move.knockaway();
    if reeling {
        return false;
    }
    saber_move.special() || (saber_move.attack() && weapon_wait_ms > 0 && blocked == Blocked::None)
}

/// How far a hilt may travel per bonus point, by style.
fn tolerance(style: Style) -> i32 {
    match style {
        Style::Strong => power::TOLERANCE[2],
        Style::Fast => power::TOLERANCE[0],
        Style::Medium | Style::Dual | Style::Staff => power::TOLERANCE[1],
    }
}

/// The attack power: the style's level, and while attacking that level
/// raised, plus a point for every stretch of the style's tolerance the
/// hilt travelled (counted in whole units, a started stretch counting
/// whole), kept within the book's bounds.
pub fn attack(style: Style, attacking: bool, hilt: Option<f32>) -> i32 {
    let mut strength = style_level(style);
    if attacking {
        strength = strength * power::ATTACKING_TIMES + power::ATTACKING_PLUS;
        if let Some(travel) = hilt {
            // A float cast saturates, so even an endless travel stays a
            // number; dividing once keeps it from overflowing.
            let units = travel as i32;
            if units > 0 {
                strength += 1 + (units - 1) / tolerance(style);
            }
        }
    }
    strength.clamp(power::LEAST, power::MOST)
}

/// The power of a style that is not attacking: its level, kept within
/// the book's bounds.
pub fn style_power(style: Style) -> i32 {
    attack(style, false, None)
}

/// When a super-break win carries its power.
#[derive(Clone, Copy)]
enum Gate {
    Always,
    Within([i32; 2]),
    Never,
}

/// The super-break wins' gates, one per new lock block, in the blocks'
/// order: each class pair (mine against theirs) side then top.
const WIN_GATES: [Gate; 18] = [
    // A single saber against the pair, the staff, a single.
    Gate::Within(breaks::S_SIDE),
    Gate::Within(breaks::S_DL_T),
    Gate::Within(breaks::S_ST_S),
    Gate::Within(breaks::S_TOP),
    Gate::Within(breaks::S_SIDE),
    Gate::Within(breaks::S_TOP),
    // The pair against the pair, the staff, a single.
    Gate::Within(breaks::DL_SIDE),
    Gate::Always,
    Gate::Within(breaks::DL_SIDE),
    Gate::Within(breaks::DL_ST_T),
    Gate::Within(breaks::DL_S_S),
    Gate::Within(breaks::DL_S_T),
    // The staff against the pair (its top win has no power, a game
    // fact), the staff, a single.
    Gate::Always,
    Gate::Never,
    Gate::Within(breaks::ST_SIDE),
    Gate::Always,
    Gate::Within(breaks::ST_SIDE),
    Gate::Always,
];

/// The new locks' animations come in blocks of five, one per class pair
/// and way: the normal break lost and won, the lock, the super-break lost
/// and won. Which block `torso` is in and its place there, if it is one.
fn new_lock_place(torso: Anim) -> Option<(usize, usize)> {
    let first = BOTH_LK_S_DL_S_B_1_L;
    torso.between(first, BOTH_LK_ST_S_T_SB_1_W).then(|| {
        let at = usize::from(torso.index() - first.index());
        // From the first block's start to the second's.
        let size = usize::from(BOTH_LK_S_DL_T_B_1_L.index() - first.index());
        (at / size, at % size)
    })
}

/// The place of one of the first block's animations in its block.
fn place_of(anim: Anim) -> usize {
    usize::from(anim.index() - BOTH_LK_S_DL_S_B_1_L.index())
}

/// Any of the eighteen super-break wins.
pub fn final_break_won(torso: Anim) -> bool {
    new_lock_place(torso).is_some_and(|(_, place)| place == place_of(BOTH_LK_S_DL_S_SB_1_W))
}

/// A super-break, won or lost.
pub fn final_break(torso: Anim) -> bool {
    final_break_won(torso) || final_break_lost(torso)
}

/// Any of the eighteen super-break losses.
pub fn final_break_lost(torso: Anim) -> bool {
    new_lock_place(torso).is_some_and(|(_, place)| place == place_of(BOTH_LK_S_DL_S_SB_1_L))
}

/// The power `torso` lends at `timer` (milliseconds left of it) to its
/// swinger, or to a defender (`defending`): some specials give a defender
/// only the defended power. A window `[end, start]` holds while at least
/// `end` ms are left and at least `start` have passed.
pub fn animation(anims: &AnimTable, torso: Anim, timer: i32, defending: bool) -> i32 {
    let elapsed = anims.get(torso).length_ms() - timer;
    let open = |[end, start]: [i32; 2]| timer >= end && elapsed >= start;
    let within = |window: [i32; 2], strength: i32| if open(window) { strength } else { 0 };
    let special = |window: [i32; 2]| within(window, power::SPECIAL);
    let defended = |otherwise: i32| {
        if defending {
            power::DEFENDED
        } else {
            otherwise
        }
    };
    let while_true = |held: bool| if held { power::SPECIAL } else { 0 };

    if torso.between(BOTH_A1_T__B_, BOTH_D1_B____) {
        return power::GROUPS[0];
    }
    if torso.between(BOTH_A2_T__B_, BOTH_D2_B____) {
        return power::GROUPS[1];
    }
    if torso.between(BOTH_A3_T__B_, BOTH_D3_B____) {
        return power::GROUPS[2];
    }
    if torso.between(BOTH_A6_T__B_, BOTH_D7_B____) {
        return power::GROUPS[3];
    }
    if torso.between(BOTH_P1_S1_T_, BOTH_H1_S1_BR) {
        return power::GROUPS[4];
    }
    match torso {
        BOTH_A2_STABBACK1 => defended(special(back::STABBACK)),
        BOTH_ATTACK_BACK => special(back::ATTACK_BACK),
        BOTH_CROUCHATTACKBACK1 => special(back::CROUCH_BACK),
        BOTH_BUTTERFLY_LEFT | BOTH_BUTTERFLY_RIGHT | BOTH_BUTTERFLY_FL1 | BOTH_BUTTERFLY_FR1 => {
            power::SPECIAL
        }
        BOTH_LUNGE2_B__T_ => defended(special(leaps::LUNGE)),
        BOTH_FORCELEAP2_T__B_ => special(leaps::FORCE_LEAP),
        BOTH_VS_ATR_S | BOTH_VS_ATL_S | BOTH_VT_ATR_S | BOTH_VT_ATL_S => power::SPECIAL,
        BOTH_JUMPFLIPSLASHDOWN1 => special(leaps::FLIP_SLASH),
        BOTH_JUMPFLIPSTABDOWN => special(leaps::FLIP_STAB),
        BOTH_JUMPATTACK6 => {
            while_true(open(two_weapons::JUMP_DUAL_FIRST) || open(two_weapons::JUMP_DUAL_SECOND))
        }
        BOTH_JUMPATTACK7 => special(leaps::BACK_FLIP),
        BOTH_SPINATTACK6 => within(spins::SPIN_DUAL, power::SPIN),
        BOTH_SPINATTACK7 => within(spins::SPIN, power::SPIN),
        BOTH_FORCELONGLEAP_ATTACK => while_true(elapsed <= leaps::LONG_LEAP),
        BOTH_STABDOWN | BOTH_STABDOWN_DUAL => while_true(timer <= down::STAB_DOWN),
        BOTH_STABDOWN_STAFF => while_true(timer <= down::STAB_DOWN_STAFF),
        BOTH_A6_SABERPROTECT => special(two_weapons::SABER_PROTECT),
        BOTH_A7_SOULCAL => special(katas::SOULCAL),
        BOTH_A1_SPECIAL => special(katas::KATA_FAST),
        BOTH_A2_SPECIAL => special(katas::KATA_MEDIUM),
        BOTH_A3_SPECIAL => special(katas::KATA_STRONG),
        BOTH_FLIP_ATTACK7 => power::SPECIAL,
        BOTH_PULL_IMPALE_STAB => defended(special(pull::PULL_STAB)),
        BOTH_PULL_IMPALE_SWING => special(pull::PULL_SWING),
        BOTH_ALORA_SPIN_SLASH => special(spins::ALORA_SPIN),
        BOTH_A6_FB | BOTH_A6_LR => defended(special(two_weapons::MIRROR)),
        BOTH_ROLL_STAB => defended(while_true(elapsed <= leaps::ROLL_STAB)),
        _ if final_break_won(torso) => {
            let block = new_lock_place(torso).map_or(0, |(block, _)| block);
            match WIN_GATES[block] {
                Gate::Always => power::SUPER_BREAK,
                Gate::Within(window) => within(window, power::SUPER_BREAK),
                Gate::Never => 0,
            }
        }
        _ => 0,
    }
}

/// The single-saber katas' bonus to animation power.
pub fn kata_bonus(torso: Anim) -> i32 {
    match torso {
        BOTH_A1_SPECIAL | BOTH_A2_SPECIAL | BOTH_A3_SPECIAL => power::KATA_BONUS,
        _ => 0,
    }
}
