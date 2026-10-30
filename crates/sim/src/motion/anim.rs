//! The animations a body plays, and how movement sets them.
//!
//! A body plays two animations at once: one on its legs and one on its
//! torso. Each part holds its animation with a timer (milliseconds left
//! before something else may take the part; −1 holds it until an override)
//! and a flip bit that toggles whenever the part starts an animation it
//! already plays, so a client sees a fresh start. Movement, the weapon step
//! and the saber's moves pick the animations through the [`Animator`]; the
//! tests at the end say which family an animation belongs to (rolls, runs,
//! landings, slope stances, knockdowns, kicks, flips, spins, stances).
//!
//! The animations' lengths are data: frame counts and frame times read from
//! the player skeleton's `animation.cfg` into an [`AnimTable`].

use std::sync::{Arc, LazyLock};

use super::MotionState;
use crate::combat::saber::Move as SaberMove;
use crate::skeleton::SkeletonData;

use Anim::*;

macro_rules! anims {
    ($($name:ident),* $(,)?) => {
        /// The animations movement uses, by their `animation.cfg` names.
        /// The numbering is ours (append only: snapshots store it).
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        #[repr(u16)]
        pub enum Anim {
            #[default]
            $($name),*
        }

        impl Anim {
            pub const ALL: &'static [Anim] = &[$(Anim::$name),*];

            /// The sequence's name in `animation.cfg`.
            pub fn name(self) -> &'static str {
                match self {
                    $(Anim::$name => stringify!($name)),*
                }
            }
        }
    };
}

anims!(
    BOTH_STAND1,
    BOTH_STAND2,
    BOTH_WALK1,
    BOTH_WALK2,
    BOTH_WALKBACK1,
    BOTH_RUN1,
    BOTH_RUNBACK1,
    BOTH_CROUCH1IDLE,
    BOTH_CROUCH1WALK,
    BOTH_CROUCH1WALKBACK,
    BOTH_JUMP1,
    BOTH_JUMPBACK1,
    BOTH_JUMPLEFT1,
    BOTH_JUMPRIGHT1,
    BOTH_INAIR1,
    BOTH_LAND1,
    BOTH_LANDBACK1,
    BOTH_ROLL_F,
    BOTH_ROLL_B,
    BOTH_ROLL_L,
    BOTH_ROLL_R,
    // The slope stances, in JA's order: left foot up by 4 to 20 units,
    // then the right; the plain stance's, then the one-handed stance's.
    LEGS_LEFTUP1,
    LEGS_LEFTUP2,
    LEGS_LEFTUP3,
    LEGS_LEFTUP4,
    LEGS_LEFTUP5,
    LEGS_RIGHTUP1,
    LEGS_RIGHTUP2,
    LEGS_RIGHTUP3,
    LEGS_RIGHTUP4,
    LEGS_RIGHTUP5,
    LEGS_S1_LUP1,
    LEGS_S1_LUP2,
    LEGS_S1_LUP3,
    LEGS_S1_LUP4,
    LEGS_S1_LUP5,
    LEGS_S1_RUP1,
    LEGS_S1_RUP2,
    LEGS_S1_RUP3,
    LEGS_S1_RUP4,
    LEGS_S1_RUP5,
    // Combat (I23): shooting, the blaster stances, pain and death.
    BOTH_ATTACK2,
    BOTH_ATTACK3,
    TORSO_WEAPONREADY2,
    TORSO_WEAPONREADY3,
    BOTH_PAIN1,
    BOTH_PAIN2,
    BOTH_PAIN3,
    BOTH_DEATH1,
    BOTH_DEATH2,
    BOTH_DEATH3,
    BOTH_DEATH4,
    BOTH_DEATH5,
    BOTH_DEATH6,
    BOTH_DEATH7,
    BOTH_DEATH8,
    BOTH_DEATH9,
    BOTH_DEATH10,
    BOTH_DEATH11,
    BOTH_DEATH12,
    BOTH_DEATH13,
    BOTH_DEATH14,
    BOTH_DEATH15,
    BOTH_DEATH16,
    BOTH_DEATH17,
    BOTH_DEATH18,
    BOTH_DEATH19,
    // The Force (I26): JA multiplayer's hand extends, and the choke.
    BOTH_FORCEPUSH,
    BOTH_FORCEPULL,
    BOTH_FORCEGRIP_HOLD,
    BOTH_FORCELIGHTNING_HOLD,
    BOTH_CHOKE3,
    // Sabers (I24). Each style's group of 77 keeps JA's order, so a move's
    // animation shifts from the fast group to its style's by one offset.
    // Stances, draw and put-away.
    BOTH_SABERFAST_STANCE,
    BOTH_SABERSLOW_STANCE,
    BOTH_SABERDUAL_STANCE,
    BOTH_SABERSTAFF_STANCE,
    BOTH_STAND1TO2,
    BOTH_STAND2TO1,
    BOTH_S1_S6,
    BOTH_S6_S1,
    BOTH_S1_S7,
    BOTH_S7_S1,
    // The fast style's attacks, transitions, starts, returns, bounces and deflections.
    BOTH_A1_T__B_,
    BOTH_A1__L__R,
    BOTH_A1__R__L,
    BOTH_A1_TL_BR,
    BOTH_A1_BR_TL,
    BOTH_A1_BL_TR,
    BOTH_A1_TR_BL,
    BOTH_T1_BR__R,
    BOTH_T1_BR_TL,
    BOTH_T1_BR__L,
    BOTH_T1_BR_BL,
    BOTH_T1__R_TR,
    BOTH_T1__R_TL,
    BOTH_T1__R__L,
    BOTH_T1__R_BL,
    BOTH_T1_TR_BR,
    BOTH_T1_TR_TL,
    BOTH_T1_TR__L,
    BOTH_T1_TR_BL,
    BOTH_T1_T__BR,
    BOTH_T1_T___R,
    BOTH_T1_T__TR,
    BOTH_T1_T__TL,
    BOTH_T1_T___L,
    BOTH_T1_T__BL,
    BOTH_T1_TL_BR,
    BOTH_T1_TL_BL,
    BOTH_T1__L_BR,
    BOTH_T1__L__R,
    BOTH_T1__L_TL,
    BOTH_T1_BL_BR,
    BOTH_T1_BL__R,
    BOTH_T1_BL_TR,
    BOTH_T1_BL__L,
    BOTH_T1_BR_TR,
    BOTH_T1_BR_T_,
    BOTH_T1__R_BR,
    BOTH_T1__R_T_,
    BOTH_T1_TR__R,
    BOTH_T1_TR_T_,
    BOTH_T1_TL__R,
    BOTH_T1_TL_TR,
    BOTH_T1_TL_T_,
    BOTH_T1_TL__L,
    BOTH_T1__L_TR,
    BOTH_T1__L_T_,
    BOTH_T1__L_BL,
    BOTH_T1_BL_T_,
    BOTH_T1_BL_TL,
    BOTH_S1_S1_T_,
    BOTH_S1_S1__L,
    BOTH_S1_S1__R,
    BOTH_S1_S1_TL,
    BOTH_S1_S1_BR,
    BOTH_S1_S1_BL,
    BOTH_S1_S1_TR,
    BOTH_R1_B__S1,
    BOTH_R1__L_S1,
    BOTH_R1__R_S1,
    BOTH_R1_TL_S1,
    BOTH_R1_BR_S1,
    BOTH_R1_BL_S1,
    BOTH_R1_TR_S1,
    BOTH_B1_BR___,
    BOTH_B1__R___,
    BOTH_B1_TR___,
    BOTH_B1_T____,
    BOTH_B1_TL___,
    BOTH_B1__L___,
    BOTH_B1_BL___,
    BOTH_D1_BR___,
    BOTH_D1__R___,
    BOTH_D1_TR___,
    BOTH_D1_TL___,
    BOTH_D1__L___,
    BOTH_D1_BL___,
    BOTH_D1_B____,
    // The medium style's attacks, transitions, starts, returns, bounces and deflections.
    BOTH_A2_T__B_,
    BOTH_A2__L__R,
    BOTH_A2__R__L,
    BOTH_A2_TL_BR,
    BOTH_A2_BR_TL,
    BOTH_A2_BL_TR,
    BOTH_A2_TR_BL,
    BOTH_T2_BR__R,
    BOTH_T2_BR_TL,
    BOTH_T2_BR__L,
    BOTH_T2_BR_BL,
    BOTH_T2__R_TR,
    BOTH_T2__R_TL,
    BOTH_T2__R__L,
    BOTH_T2__R_BL,
    BOTH_T2_TR_BR,
    BOTH_T2_TR_TL,
    BOTH_T2_TR__L,
    BOTH_T2_TR_BL,
    BOTH_T2_T__BR,
    BOTH_T2_T___R,
    BOTH_T2_T__TR,
    BOTH_T2_T__TL,
    BOTH_T2_T___L,
    BOTH_T2_T__BL,
    BOTH_T2_TL_BR,
    BOTH_T2_TL_BL,
    BOTH_T2__L_BR,
    BOTH_T2__L__R,
    BOTH_T2__L_TL,
    BOTH_T2_BL_BR,
    BOTH_T2_BL__R,
    BOTH_T2_BL_TR,
    BOTH_T2_BL__L,
    BOTH_T2_BR_TR,
    BOTH_T2_BR_T_,
    BOTH_T2__R_BR,
    BOTH_T2__R_T_,
    BOTH_T2_TR__R,
    BOTH_T2_TR_T_,
    BOTH_T2_TL__R,
    BOTH_T2_TL_TR,
    BOTH_T2_TL_T_,
    BOTH_T2_TL__L,
    BOTH_T2__L_TR,
    BOTH_T2__L_T_,
    BOTH_T2__L_BL,
    BOTH_T2_BL_T_,
    BOTH_T2_BL_TL,
    BOTH_S2_S1_T_,
    BOTH_S2_S1__L,
    BOTH_S2_S1__R,
    BOTH_S2_S1_TL,
    BOTH_S2_S1_BR,
    BOTH_S2_S1_BL,
    BOTH_S2_S1_TR,
    BOTH_R2_B__S1,
    BOTH_R2__L_S1,
    BOTH_R2__R_S1,
    BOTH_R2_TL_S1,
    BOTH_R2_BR_S1,
    BOTH_R2_BL_S1,
    BOTH_R2_TR_S1,
    BOTH_B2_BR___,
    BOTH_B2__R___,
    BOTH_B2_TR___,
    BOTH_B2_T____,
    BOTH_B2_TL___,
    BOTH_B2__L___,
    BOTH_B2_BL___,
    BOTH_D2_BR___,
    BOTH_D2__R___,
    BOTH_D2_TR___,
    BOTH_D2_TL___,
    BOTH_D2__L___,
    BOTH_D2_BL___,
    BOTH_D2_B____,
    // The strong style's attacks, transitions, starts, returns, bounces and deflections.
    BOTH_A3_T__B_,
    BOTH_A3__L__R,
    BOTH_A3__R__L,
    BOTH_A3_TL_BR,
    BOTH_A3_BR_TL,
    BOTH_A3_BL_TR,
    BOTH_A3_TR_BL,
    BOTH_T3_BR__R,
    BOTH_T3_BR_TL,
    BOTH_T3_BR__L,
    BOTH_T3_BR_BL,
    BOTH_T3__R_TR,
    BOTH_T3__R_TL,
    BOTH_T3__R__L,
    BOTH_T3__R_BL,
    BOTH_T3_TR_BR,
    BOTH_T3_TR_TL,
    BOTH_T3_TR__L,
    BOTH_T3_TR_BL,
    BOTH_T3_T__BR,
    BOTH_T3_T___R,
    BOTH_T3_T__TR,
    BOTH_T3_T__TL,
    BOTH_T3_T___L,
    BOTH_T3_T__BL,
    BOTH_T3_TL_BR,
    BOTH_T3_TL_BL,
    BOTH_T3__L_BR,
    BOTH_T3__L__R,
    BOTH_T3__L_TL,
    BOTH_T3_BL_BR,
    BOTH_T3_BL__R,
    BOTH_T3_BL_TR,
    BOTH_T3_BL__L,
    BOTH_T3_BR_TR,
    BOTH_T3_BR_T_,
    BOTH_T3__R_BR,
    BOTH_T3__R_T_,
    BOTH_T3_TR__R,
    BOTH_T3_TR_T_,
    BOTH_T3_TL__R,
    BOTH_T3_TL_TR,
    BOTH_T3_TL_T_,
    BOTH_T3_TL__L,
    BOTH_T3__L_TR,
    BOTH_T3__L_T_,
    BOTH_T3__L_BL,
    BOTH_T3_BL_T_,
    BOTH_T3_BL_TL,
    BOTH_S3_S1_T_,
    BOTH_S3_S1__L,
    BOTH_S3_S1__R,
    BOTH_S3_S1_TL,
    BOTH_S3_S1_BR,
    BOTH_S3_S1_BL,
    BOTH_S3_S1_TR,
    BOTH_R3_B__S1,
    BOTH_R3__L_S1,
    BOTH_R3__R_S1,
    BOTH_R3_TL_S1,
    BOTH_R3_BR_S1,
    BOTH_R3_BL_S1,
    BOTH_R3_TR_S1,
    BOTH_B3_BR___,
    BOTH_B3__R___,
    BOTH_B3_TR___,
    BOTH_B3_T____,
    BOTH_B3_TL___,
    BOTH_B3__L___,
    BOTH_B3_BL___,
    BOTH_D3_BR___,
    BOTH_D3__R___,
    BOTH_D3_TR___,
    BOTH_D3_TL___,
    BOTH_D3__L___,
    BOTH_D3_BL___,
    BOTH_D3_B____,
    // The dual style's attacks, transitions, starts, returns, bounces and deflections.
    BOTH_A6_T__B_,
    BOTH_A6__L__R,
    BOTH_A6__R__L,
    BOTH_A6_TL_BR,
    BOTH_A6_BR_TL,
    BOTH_A6_BL_TR,
    BOTH_A6_TR_BL,
    BOTH_T6_BR__R,
    BOTH_T6_BR_TL,
    BOTH_T6_BR__L,
    BOTH_T6_BR_BL,
    BOTH_T6__R_TR,
    BOTH_T6__R_TL,
    BOTH_T6__R__L,
    BOTH_T6__R_BL,
    BOTH_T6_TR_BR,
    BOTH_T6_TR_TL,
    BOTH_T6_TR__L,
    BOTH_T6_TR_BL,
    BOTH_T6_T__BR,
    BOTH_T6_T___R,
    BOTH_T6_T__TR,
    BOTH_T6_T__TL,
    BOTH_T6_T___L,
    BOTH_T6_T__BL,
    BOTH_T6_TL_BR,
    BOTH_T6_TL_BL,
    BOTH_T6__L_BR,
    BOTH_T6__L__R,
    BOTH_T6__L_TL,
    BOTH_T6_BL_BR,
    BOTH_T6_BL__R,
    BOTH_T6_BL_TR,
    BOTH_T6_BL__L,
    BOTH_T6_BR_TR,
    BOTH_T6_BR_T_,
    BOTH_T6__R_BR,
    BOTH_T6__R_T_,
    BOTH_T6_TR__R,
    BOTH_T6_TR_T_,
    BOTH_T6_TL__R,
    BOTH_T6_TL_TR,
    BOTH_T6_TL_T_,
    BOTH_T6_TL__L,
    BOTH_T6__L_TR,
    BOTH_T6__L_T_,
    BOTH_T6__L_BL,
    BOTH_T6_BL_T_,
    BOTH_T6_BL_TL,
    BOTH_S6_S6_T_,
    BOTH_S6_S6__L,
    BOTH_S6_S6__R,
    BOTH_S6_S6_TL,
    BOTH_S6_S6_BR,
    BOTH_S6_S6_BL,
    BOTH_S6_S6_TR,
    BOTH_R6_B__S6,
    BOTH_R6__L_S6,
    BOTH_R6__R_S6,
    BOTH_R6_TL_S6,
    BOTH_R6_BR_S6,
    BOTH_R6_BL_S6,
    BOTH_R6_TR_S6,
    BOTH_B6_BR___,
    BOTH_B6__R___,
    BOTH_B6_TR___,
    BOTH_B6_T____,
    BOTH_B6_TL___,
    BOTH_B6__L___,
    BOTH_B6_BL___,
    BOTH_D6_BR___,
    BOTH_D6__R___,
    BOTH_D6_TR___,
    BOTH_D6_TL___,
    BOTH_D6__L___,
    BOTH_D6_BL___,
    BOTH_D6_B____,
    // The staff style's attacks, transitions, starts, returns, bounces and deflections.
    BOTH_A7_T__B_,
    BOTH_A7__L__R,
    BOTH_A7__R__L,
    BOTH_A7_TL_BR,
    BOTH_A7_BR_TL,
    BOTH_A7_BL_TR,
    BOTH_A7_TR_BL,
    BOTH_T7_BR__R,
    BOTH_T7_BR_TL,
    BOTH_T7_BR__L,
    BOTH_T7_BR_BL,
    BOTH_T7__R_TR,
    BOTH_T7__R_TL,
    BOTH_T7__R__L,
    BOTH_T7__R_BL,
    BOTH_T7_TR_BR,
    BOTH_T7_TR_TL,
    BOTH_T7_TR__L,
    BOTH_T7_TR_BL,
    BOTH_T7_T__BR,
    BOTH_T7_T___R,
    BOTH_T7_T__TR,
    BOTH_T7_T__TL,
    BOTH_T7_T___L,
    BOTH_T7_T__BL,
    BOTH_T7_TL_BR,
    BOTH_T7_TL_BL,
    BOTH_T7__L_BR,
    BOTH_T7__L__R,
    BOTH_T7__L_TL,
    BOTH_T7_BL_BR,
    BOTH_T7_BL__R,
    BOTH_T7_BL_TR,
    BOTH_T7_BL__L,
    BOTH_T7_BR_TR,
    BOTH_T7_BR_T_,
    BOTH_T7__R_BR,
    BOTH_T7__R_T_,
    BOTH_T7_TR__R,
    BOTH_T7_TR_T_,
    BOTH_T7_TL__R,
    BOTH_T7_TL_TR,
    BOTH_T7_TL_T_,
    BOTH_T7_TL__L,
    BOTH_T7__L_TR,
    BOTH_T7__L_T_,
    BOTH_T7__L_BL,
    BOTH_T7_BL_T_,
    BOTH_T7_BL_TL,
    BOTH_S7_S7_T_,
    BOTH_S7_S7__L,
    BOTH_S7_S7__R,
    BOTH_S7_S7_TL,
    BOTH_S7_S7_BR,
    BOTH_S7_S7_BL,
    BOTH_S7_S7_TR,
    BOTH_R7_B__S7,
    BOTH_R7__L_S7,
    BOTH_R7__R_S7,
    BOTH_R7_TL_S7,
    BOTH_R7_BR_S7,
    BOTH_R7_BL_S7,
    BOTH_R7_TR_S7,
    BOTH_B7_BR___,
    BOTH_B7__R___,
    BOTH_B7_TR___,
    BOTH_B7_T____,
    BOTH_B7_TL___,
    BOTH_B7__L___,
    BOTH_B7_BL___,
    BOTH_D7_BR___,
    BOTH_D7__R___,
    BOTH_D7_TR___,
    BOTH_D7_TL___,
    BOTH_D7__L___,
    BOTH_D7_BL___,
    BOTH_D7_B____,
    // Parries, knockaways, broken attacks and broken parries: single, dual, staff.
    BOTH_P1_S1_T_,
    BOTH_P1_S1_TR,
    BOTH_P1_S1_TL,
    BOTH_P1_S1_BL,
    BOTH_P1_S1_BR,
    BOTH_K1_S1_T_,
    BOTH_K1_S1_TR,
    BOTH_K1_S1_TL,
    BOTH_K1_S1_BL,
    BOTH_K1_S1_B_,
    BOTH_K1_S1_BR,
    BOTH_V1_BR_S1,
    BOTH_V1__R_S1,
    BOTH_V1_TR_S1,
    BOTH_V1_T__S1,
    BOTH_V1_TL_S1,
    BOTH_V1__L_S1,
    BOTH_V1_BL_S1,
    BOTH_V1_B__S1,
    BOTH_H1_S1_T_,
    BOTH_H1_S1_TR,
    BOTH_H1_S1_TL,
    BOTH_H1_S1_BL,
    BOTH_H1_S1_B_,
    BOTH_H1_S1_BR,
    BOTH_P6_S6_T_,
    BOTH_P6_S6_TR,
    BOTH_P6_S6_TL,
    BOTH_P6_S6_BL,
    BOTH_P6_S6_BR,
    BOTH_K6_S6_T_,
    BOTH_K6_S6_TR,
    BOTH_K6_S6_TL,
    BOTH_K6_S6_BL,
    BOTH_K6_S6_B_,
    BOTH_K6_S6_BR,
    BOTH_V6_BR_S6,
    BOTH_V6__R_S6,
    BOTH_V6_TR_S6,
    BOTH_V6_T__S6,
    BOTH_V6_TL_S6,
    BOTH_V6__L_S6,
    BOTH_V6_BL_S6,
    BOTH_V6_B__S6,
    BOTH_H6_S6_T_,
    BOTH_H6_S6_TR,
    BOTH_H6_S6_TL,
    BOTH_H6_S6_BL,
    BOTH_H6_S6_B_,
    BOTH_H6_S6_BR,
    BOTH_P7_S7_T_,
    BOTH_P7_S7_TR,
    BOTH_P7_S7_TL,
    BOTH_P7_S7_BL,
    BOTH_P7_S7_BR,
    BOTH_K7_S7_T_,
    BOTH_K7_S7_TR,
    BOTH_K7_S7_TL,
    BOTH_K7_S7_BL,
    BOTH_K7_S7_B_,
    BOTH_K7_S7_BR,
    BOTH_V7_BR_S7,
    BOTH_V7__R_S7,
    BOTH_V7_TR_S7,
    BOTH_V7_T__S7,
    BOTH_V7_TL_S7,
    BOTH_V7__L_S7,
    BOTH_V7_BL_S7,
    BOTH_V7_B__S7,
    BOTH_H7_S7_T_,
    BOTH_H7_S7_TR,
    BOTH_H7_S7_TL,
    BOTH_H7_S7_BL,
    BOTH_H7_S7_B_,
    BOTH_H7_S7_BR,
    // The specials.
    BOTH_A2_STABBACK1,
    BOTH_ATTACK_BACK,
    BOTH_CROUCHATTACKBACK1,
    BOTH_ROLL_STAB,
    BOTH_LUNGE2_B__T_,
    BOTH_FORCELEAP2_T__B_,
    BOTH_JUMPFLIPSTABDOWN,
    BOTH_JUMPFLIPSLASHDOWN1,
    BOTH_JUMPATTACK6,
    BOTH_ARIAL_LEFT,
    BOTH_ARIAL_RIGHT,
    BOTH_CARTWHEEL_LEFT,
    BOTH_CARTWHEEL_RIGHT,
    BOTH_BUTTERFLY_FL1,
    BOTH_BUTTERFLY_FR1,
    BOTH_BUTTERFLY_LEFT,
    BOTH_BUTTERFLY_RIGHT,
    BOTH_JUMPATTACK7,
    BOTH_SPINATTACK6,
    BOTH_SPINATTACK7,
    BOTH_FORCELONGLEAP_ATTACK,
    BOTH_VS_ATR_S,
    BOTH_VS_ATL_S,
    BOTH_VT_ATR_S,
    BOTH_VT_ATL_S,
    BOTH_A7_KICK_F,
    BOTH_A7_KICK_B,
    BOTH_A7_KICK_R,
    BOTH_A7_KICK_L,
    BOTH_A7_KICK_S,
    BOTH_A7_KICK_BF,
    BOTH_A7_KICK_RL,
    BOTH_A7_KICK_F_AIR,
    BOTH_A7_KICK_B_AIR,
    BOTH_A7_KICK_R_AIR,
    BOTH_A7_KICK_L_AIR,
    BOTH_STABDOWN,
    BOTH_STABDOWN_STAFF,
    BOTH_STABDOWN_DUAL,
    BOTH_A6_SABERPROTECT,
    BOTH_A7_SOULCAL,
    BOTH_A1_SPECIAL,
    BOTH_A2_SPECIAL,
    BOTH_A3_SPECIAL,
    BOTH_FLIP_ATTACK7,
    BOTH_PULL_IMPALE_STAB,
    BOTH_PULL_IMPALE_SWING,
    BOTH_ALORA_SPIN_SLASH,
    BOTH_A6_FB,
    BOTH_A6_LR,
    BOTH_A7_HILT,
    // Walking and running with a lit saber.
    BOTH_RUN2,
    BOTH_RUNBACK2,
    BOTH_WALKBACK2,
    BOTH_RUN_STAFF,
    BOTH_RUN_DUAL,
    BOTH_WALK_STAFF,
    BOTH_WALK_DUAL,
    BOTH_WALKBACK_STAFF,
    BOTH_WALKBACK_DUAL,
    // Knocked down, getting up; changing weapons.
    BOTH_KNOCKDOWN1,
    BOTH_GETUP1,
    BOTH_FORCE_GETUP_B1,
    BOTH_FORCE_GETUP_B3,
    BOTH_GETUP_BROLL_B,
    BOTH_GETUP_BROLL_F,
    BOTH_GETUP_BROLL_L,
    BOTH_GETUP_BROLL_R,
    TORSO_DROPWEAP1,
    TORSO_RAISEWEAP1,
    // Force jump (I26): the flips and JA's directional force jumps.
    BOTH_FLIP_F,
    BOTH_FLIP_B,
    BOTH_FLIP_L,
    BOTH_FLIP_R,
    BOTH_FORCEJUMP1,
    BOTH_FORCEJUMPBACK1,
    BOTH_FORCEJUMPLEFT1,
    BOTH_FORCEJUMPRIGHT1,
    BOTH_FORCELAND1,
    BOTH_FORCELANDBACK1,
    BOTH_FORCELANDLEFT1,
    BOTH_FORCELANDRIGHT1,
    // Saber locks (I24): JA's old single-saber locks and their breaks,
    // then the new locks, by my style and theirs, side then top, each
    // as the normal break lost and won, the lock, the super-break lost
    // and won (JA's order: a lock's animation less two is its lost
    // break); the other-started variants last. Thrown, the pull pose;
    // lost, the fall.
    BOTH_LK_S_DL_S_B_1_L,
    BOTH_LK_S_DL_S_B_1_W,
    BOTH_LK_S_DL_S_L_1,
    BOTH_LK_S_DL_S_SB_1_L,
    BOTH_LK_S_DL_S_SB_1_W,
    BOTH_LK_S_DL_T_B_1_L,
    BOTH_LK_S_DL_T_B_1_W,
    BOTH_LK_S_DL_T_L_1,
    BOTH_LK_S_DL_T_SB_1_L,
    BOTH_LK_S_DL_T_SB_1_W,
    BOTH_LK_S_ST_S_B_1_L,
    BOTH_LK_S_ST_S_B_1_W,
    BOTH_LK_S_ST_S_L_1,
    BOTH_LK_S_ST_S_SB_1_L,
    BOTH_LK_S_ST_S_SB_1_W,
    BOTH_LK_S_ST_T_B_1_L,
    BOTH_LK_S_ST_T_B_1_W,
    BOTH_LK_S_ST_T_L_1,
    BOTH_LK_S_ST_T_SB_1_L,
    BOTH_LK_S_ST_T_SB_1_W,
    BOTH_LK_S_S_S_B_1_L,
    BOTH_LK_S_S_S_B_1_W,
    BOTH_LK_S_S_S_L_1,
    BOTH_LK_S_S_S_SB_1_L,
    BOTH_LK_S_S_S_SB_1_W,
    BOTH_LK_S_S_T_B_1_L,
    BOTH_LK_S_S_T_B_1_W,
    BOTH_LK_S_S_T_L_1,
    BOTH_LK_S_S_T_SB_1_L,
    BOTH_LK_S_S_T_SB_1_W,
    BOTH_LK_DL_DL_S_B_1_L,
    BOTH_LK_DL_DL_S_B_1_W,
    BOTH_LK_DL_DL_S_L_1,
    BOTH_LK_DL_DL_S_SB_1_L,
    BOTH_LK_DL_DL_S_SB_1_W,
    BOTH_LK_DL_DL_T_B_1_L,
    BOTH_LK_DL_DL_T_B_1_W,
    BOTH_LK_DL_DL_T_L_1,
    BOTH_LK_DL_DL_T_SB_1_L,
    BOTH_LK_DL_DL_T_SB_1_W,
    BOTH_LK_DL_ST_S_B_1_L,
    BOTH_LK_DL_ST_S_B_1_W,
    BOTH_LK_DL_ST_S_L_1,
    BOTH_LK_DL_ST_S_SB_1_L,
    BOTH_LK_DL_ST_S_SB_1_W,
    BOTH_LK_DL_ST_T_B_1_L,
    BOTH_LK_DL_ST_T_B_1_W,
    BOTH_LK_DL_ST_T_L_1,
    BOTH_LK_DL_ST_T_SB_1_L,
    BOTH_LK_DL_ST_T_SB_1_W,
    BOTH_LK_DL_S_S_B_1_L,
    BOTH_LK_DL_S_S_B_1_W,
    BOTH_LK_DL_S_S_L_1,
    BOTH_LK_DL_S_S_SB_1_L,
    BOTH_LK_DL_S_S_SB_1_W,
    BOTH_LK_DL_S_T_B_1_L,
    BOTH_LK_DL_S_T_B_1_W,
    BOTH_LK_DL_S_T_L_1,
    BOTH_LK_DL_S_T_SB_1_L,
    BOTH_LK_DL_S_T_SB_1_W,
    BOTH_LK_ST_DL_S_B_1_L,
    BOTH_LK_ST_DL_S_B_1_W,
    BOTH_LK_ST_DL_S_L_1,
    BOTH_LK_ST_DL_S_SB_1_L,
    BOTH_LK_ST_DL_S_SB_1_W,
    BOTH_LK_ST_DL_T_B_1_L,
    BOTH_LK_ST_DL_T_B_1_W,
    BOTH_LK_ST_DL_T_L_1,
    BOTH_LK_ST_DL_T_SB_1_L,
    BOTH_LK_ST_DL_T_SB_1_W,
    BOTH_LK_ST_ST_S_B_1_L,
    BOTH_LK_ST_ST_S_B_1_W,
    BOTH_LK_ST_ST_S_L_1,
    BOTH_LK_ST_ST_S_SB_1_L,
    BOTH_LK_ST_ST_S_SB_1_W,
    BOTH_LK_ST_ST_T_B_1_L,
    BOTH_LK_ST_ST_T_B_1_W,
    BOTH_LK_ST_ST_T_L_1,
    BOTH_LK_ST_ST_T_SB_1_L,
    BOTH_LK_ST_ST_T_SB_1_W,
    BOTH_LK_ST_S_S_B_1_L,
    BOTH_LK_ST_S_S_B_1_W,
    BOTH_LK_ST_S_S_L_1,
    BOTH_LK_ST_S_S_SB_1_L,
    BOTH_LK_ST_S_S_SB_1_W,
    BOTH_LK_ST_S_T_B_1_L,
    BOTH_LK_ST_S_T_B_1_W,
    BOTH_LK_ST_S_T_L_1,
    BOTH_LK_ST_S_T_SB_1_L,
    BOTH_LK_ST_S_T_SB_1_W,
    BOTH_LK_S_S_S_L_2,
    BOTH_LK_S_S_T_L_2,
    BOTH_LK_DL_DL_S_L_2,
    BOTH_LK_DL_DL_T_L_2,
    BOTH_LK_ST_ST_S_L_2,
    BOTH_LK_ST_ST_T_L_2,
    BOTH_BF2BREAK,
    BOTH_BF2LOCK,
    BOTH_BF1BREAK,
    BOTH_BF1LOCK,
    BOTH_CWCIRCLEBREAK,
    BOTH_CCWCIRCLEBREAK,
    BOTH_CWCIRCLELOCK,
    BOTH_CCWCIRCLELOCK,
    BOTH_KNOCKDOWN4,
    BOTH_SABERPULL,
    BOTH_STAND1IDLE1,
    BOTH_STAND2IDLE1,
    BOTH_STAND2IDLE2,
    // Fists: the left punch, then the right.
    BOTH_MELEE1,
    BOTH_MELEE2,
    // Getting up from a death lain in (`combat::death::getup`): on the back,
    // on the front.
    BOTH_GETUP4,
    BOTH_GETUP3,
);

impl Anim {
    pub fn index(self) -> u16 {
        self as u16
    }

    pub fn from_index(index: u16) -> Option<Anim> {
        Anim::ALL.get(usize::from(index)).copied()
    }

    pub fn from_name(name: &str) -> Option<Anim> {
        Anim::ALL
            .iter()
            .copied()
            .find(|anim| anim.name().eq_ignore_ascii_case(name))
    }

    /// `offset` places further in the list (slope stances step so).
    pub(crate) fn offset(self, offset: i32) -> Anim {
        let index = i32::from(self.index()) + offset;
        u16::try_from(index)
            .ok()
            .and_then(Anim::from_index)
            .unwrap_or(self)
    }

    pub(crate) fn between(self, first: Anim, last: Anim) -> bool {
        (first..=last).contains(&self)
    }
}

/// One sequence of `animation.cfg` as movement needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimInfo {
    pub first_frame: i32,
    pub frames: i32,
    /// Milliseconds per frame, negative for a sequence played backward
    /// (`floor(1000 / fps)` backward, `ceil` forward).
    pub frame_lerp: i32,
    /// Where each sole's tag stands in the sequence's first frame (left,
    /// right), in units in the model's frame (+X forward, +Y left): the
    /// slope stances trace down from them. JA reads the tags off the
    /// animated skeleton each move; `None` skips the slope stances.
    pub feet: Option<[[f32; 2]; 2]>,
    /// The sequence loops (`loopFrames` set): the sim's skeleton plays it
    /// round (`sim::skeleton::Sequence`).
    pub looping: bool,
}

impl AnimInfo {
    /// A sequence the skeleton lacks.
    pub const MISSING: AnimInfo = AnimInfo {
        first_frame: 0,
        frames: 0,
        frame_lerp: 100,
        feet: None,
        looping: false,
    };

    pub(crate) fn missing(&self) -> bool {
        self.first_frame == 0 && self.frames == 0
    }

    /// A held animation's timer: every frame's time.
    pub fn length_ms(&self) -> i32 {
        self.frames * self.frame_lerp.abs()
    }

    /// A saber move's held timer at `speed`: every frame but the first,
    /// shortened by the whole milliseconds `speed` saves (not divided by
    /// it: QUIRKS.md Q-0008), less one; one frame when that leaves nothing.
    pub fn move_hold_ms(&self, speed: f32) -> i32 {
        let frame = self.frame_lerp.abs();
        let hold = (self.frames - 1) * frame;
        let saved = (hold as f32 - hold as f32 * speed) as i32;
        let stretched = hold + saved;
        if stretched > 1 { stretched - 1 } else { frame }
    }
}

/// The lengths of the movement animations on the player skeleton, and,
/// when the host has JA's install, the skeleton itself: the bones the hands
/// hang from and the sabers' blade tags (`sim::skeleton`), which place the
/// blades for hits, blocks and clashes (`combat::saber::blades`). Both come
/// from the same `_humanoid` model, so they travel together to every level.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimTable {
    infos: Vec<AnimInfo>,
    skeleton: Option<Arc<SkeletonData>>,
}

impl AnimTable {
    /// Each animation looked up by name; `None` for one the file lacks.
    pub fn from_lookup(mut lookup: impl FnMut(&str) -> Option<AnimInfo>) -> AnimTable {
        AnimTable {
            infos: Anim::ALL
                .iter()
                .map(|anim| lookup(anim.name()).unwrap_or(AnimInfo::MISSING))
                .collect(),
            skeleton: None,
        }
    }

    /// This table with the skeleton its animations play on.
    pub fn with_skeleton(self, skeleton: Arc<SkeletonData>) -> AnimTable {
        AnimTable {
            skeleton: Some(skeleton),
            ..self
        }
    }

    /// This table without its skeleton (no blade has a place: the blades'
    /// cost is what a step spends beyond this one's).
    pub fn without_skeleton(self) -> AnimTable {
        AnimTable {
            skeleton: None,
            ..self
        }
    }

    /// The skeleton, if the host had it: without one no blade has a place,
    /// so blades hit, block and clash nothing (runs without JA's data).
    pub fn skeleton(&self) -> Option<&SkeletonData> {
        self.skeleton.as_deref()
    }

    /// A made-up table for runs without JA's data (the placeholder world,
    /// CI): every animation 20 frames of 50 ms, feet 6 units either side.
    pub fn stand_in() -> AnimTable {
        AnimTable::from_lookup(|_| {
            Some(AnimInfo {
                first_frame: 1,
                frames: 20,
                frame_lerp: 50,
                feet: Some([[0.0, 6.0], [0.0, -6.0]]),
                looping: false,
            })
        })
    }

    pub fn get(&self, anim: Anim) -> &AnimInfo {
        &self.infos[usize::from(anim.index())]
    }

    /// BLAKE3 of every length and sole position as bits, and the
    /// skeleton's digest: two machines with the same digest time movement
    /// and place blades the same.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = collision::Digester::new();
        for info in &self.infos {
            for value in [info.first_frame, info.frames, info.frame_lerp] {
                hasher.update(&value.to_le_bytes());
            }
            hasher.update(&[u8::from(info.looping)]);
            match info.feet {
                Some(feet) => {
                    hasher.update(&[1]);
                    for value in feet.iter().flatten() {
                        hasher.update(&value.to_bits().to_le_bytes());
                    }
                }
                None => {
                    hasher.update(&[0]);
                }
            }
        }
        if let Some(skeleton) = &self.skeleton {
            hasher.update(b"skeleton");
            hasher.update(&skeleton.digest());
        }
        *hasher.finalize().as_bytes()
    }
}

/// Which parts of the body an animation is set on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Parts(u8);

impl Parts {
    pub const UPPER: Parts = Parts(1);
    pub const LOWER: Parts = Parts(2);
    pub const WHOLE: Parts = Parts(3);

    fn includes(self, part: Parts) -> bool {
        self.0 & part.0 != 0
    }
}

/// How an animation is set (bits, or-ed together).
pub(super) mod play_flags {
    /// Only on a part whose timer has run out.
    pub const WHEN_FREE: u8 = 0;
    /// Over whatever the part plays, timer or not.
    pub const OVER_ANY: u8 = 1;
    /// The part's timer becomes the animation's held time.
    pub const HOLD_FULL: u8 = 2;
    /// Start again even when the part already plays the animation.
    pub const ANEW: u8 = 4;
    /// The held time is a saber move's (`AnimInfo::move_hold_ms`), not the
    /// whole length.
    pub const HOLD_MOVE: u8 = 8;
}

/// What one part of the body plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Part {
    pub anim: Anim,
    /// Milliseconds before another animation may take the part; −1 holds it
    /// until an override.
    pub timer: i32,
    /// Toggled at every start of the animation the part already plays (or
    /// last sent), so a client sees it start over.
    pub flip: bool,
}

impl Part {
    /// The part starts `anim`; `sent` is what the last snapshot showed it
    /// playing. The timer is left alone.
    fn start(&mut self, anim: Anim, sent: Anim) {
        if self.anim == anim || sent == anim {
            self.flip = !self.flip;
        }
        self.anim = anim;
    }
}

impl MotionState {
    /// The legs start `anim`, never while their timer runs (every caller
    /// makes sure of that).
    fn begin_legs(&mut self, anim: Anim, sent: Anim) {
        if self.legs.timer > 0 {
            return;
        }
        self.legs.start(anim, sent);
    }

    /// The torso starts `anim`; `sent` is the torso's animation as the last
    /// snapshot showed it.
    pub(super) fn begin_torso(&mut self, anim: Anim, sent: Anim) {
        self.torso.start(anim, sent);
    }
}

/// Sets animations on a body for one step: the lengths, and what the last
/// snapshot showed each part playing (for the flip bits).
pub(super) struct Animator<'a> {
    pub table: &'a AnimTable,
    pub sent_legs: Anim,
    pub sent_torso: Anim,
}

impl Animator<'_> {
    /// Sets `anim` on `parts` with the flags `set`, at normal speed.
    pub fn play(&self, ps: &mut MotionState, parts: Parts, anim: Anim, set: u8) {
        self.play_at(ps, parts, anim, set, 1.0);
    }

    /// Sets `anim` on `parts` with the flags `set`; `speed` only shortens a
    /// saber move's held time (`play_flags::HOLD_MOVE`).
    pub fn play_at(&self, ps: &mut MotionState, parts: Parts, anim: Anim, set: u8, speed: f32) {
        let has = |flag: u8| set & flag != 0;

        // A skeleton without the backward walk and runs or the plain run
        // walks forward instead; any other missing animation is not set.
        let mut anim = anim;
        if self.table.get(anim).missing()
            && matches!(anim, BOTH_RUNBACK1 | BOTH_WALKBACK1 | BOTH_RUN1)
        {
            anim = BOTH_WALK2;
        }
        let info = self.table.get(anim);
        if info.missing() {
            return;
        }

        let torso = parts.includes(Parts::UPPER);
        let legs = parts.includes(Parts::LOWER);

        if has(play_flags::OVER_ANY) {
            for (asked, part) in [(torso, &mut ps.torso), (legs, &mut ps.legs)] {
                if asked && (has(play_flags::ANEW) || part.anim != anim) {
                    part.timer = 0;
                }
            }
        }

        let held = if has(play_flags::HOLD_MOVE) {
            info.move_hold_ms(speed)
        } else {
            info.length_ms()
        };
        // A part takes the animation when it is new to it (or restarted)
        // and either the set overrides or the part's own timer has run out
        // (−1 holds until an override).
        let takes = |part: &Part| {
            (has(play_flags::ANEW) || part.anim != anim)
                && (has(play_flags::OVER_ANY) || (part.timer <= 0 && part.timer != -1))
        };

        if torso && takes(&ps.torso) {
            ps.begin_torso(anim, self.sent_torso);
            if has(play_flags::HOLD_FULL) {
                ps.torso.timer = held;
            }
        }
        if legs && takes(&ps.legs) {
            ps.begin_legs(anim, self.sent_legs);
            if has(play_flags::HOLD_FULL) {
                ps.legs.timer = held;
            }
        }
    }

    /// The legs go on to `anim` once their timer is out, unless they
    /// already play it.
    pub fn legs_next(&self, ps: &mut MotionState, anim: Anim) {
        if ps.legs.anim == anim || ps.legs.timer > 0 {
            return;
        }
        ps.begin_legs(anim, self.sent_legs);
    }

    /// The legs play `anim` now, whatever their timer, unless that would
    /// cut a roll short for something that is not one.
    pub fn legs_now(&self, ps: &mut MotionState, anim: Anim) {
        if roll_playing(ps, ps.legs.anim) && !roll_playing(ps, anim) {
            return;
        }
        ps.legs.timer = 0;
        ps.begin_legs(anim, self.sent_legs);
    }
}

// The animation tests. Each family is worked out once, from its rule over
// the names (or from the saber move table), into a set of bits.

const WORDS: usize = Anim::ALL.len().div_ceil(64);

/// A set of animations, one bit each by index.
#[derive(Clone, Copy)]
struct AnimSet([u64; WORDS]);

impl AnimSet {
    fn of(test: impl Fn(Anim) -> bool) -> AnimSet {
        let mut set = AnimSet([0; WORDS]);
        for &anim in Anim::ALL {
            if test(anim) {
                set.add(anim);
            }
        }
        set
    }

    fn named(names: &[&str]) -> AnimSet {
        let mut set = AnimSet([0; WORDS]);
        for name in names {
            let anim =
                Anim::from_name(name).unwrap_or_else(|| panic!("no animation is called {name}"));
            set.add(anim);
        }
        set
    }

    fn add(&mut self, anim: Anim) {
        let i = usize::from(anim.index());
        self.0[i / 64] |= 1 << (i % 64);
    }

    fn union(self, other: AnimSet) -> AnimSet {
        let mut both = self;
        for (word, more) in both.0.iter_mut().zip(other.0) {
            *word |= more;
        }
        both
    }

    fn has(&self, anim: Anim) -> bool {
        let i = usize::from(anim.index());
        self.0[i / 64] & (1 << (i % 64)) != 0
    }
}

struct Families {
    running: AnimSet,
    landing: AnimSet,
    slope: AnimSet,
    knockdown: AnimSet,
    kicking: AnimSet,
    kata: AnimSet,
    flipping: AnimSet,
    special_jump: AnimSet,
    jumping: AnimSet,
    special_attack: AnimSet,
    spinning: AnimSet,
    single_transition: AnimSet,
}

static FAMILIES: LazyLock<Families> = LazyLock::new(Families::build);

impl Families {
    fn build() -> Families {
        let starts = |prefix: &'static str| AnimSet::of(move |a| a.name().starts_with(prefix));

        let flipping = AnimSet::named(&[
            "BOTH_A7_SOULCAL",
            "BOTH_ARIAL_LEFT",
            "BOTH_ARIAL_RIGHT",
            "BOTH_BUTTERFLY_FL1",
            "BOTH_BUTTERFLY_FR1",
            "BOTH_BUTTERFLY_LEFT",
            "BOTH_BUTTERFLY_RIGHT",
            "BOTH_CARTWHEEL_LEFT",
            "BOTH_CARTWHEEL_RIGHT",
            "BOTH_FLIP_ATTACK7",
            "BOTH_JUMPATTACK6",
            "BOTH_JUMPATTACK7",
            "BOTH_JUMPFLIPSLASHDOWN1",
            "BOTH_JUMPFLIPSTABDOWN",
        ]);
        let moves = || SaberMove::ALL.iter().copied();
        let side_moves = [
            BOTH_ARIAL_LEFT,
            BOTH_ARIAL_RIGHT,
            BOTH_CARTWHEEL_LEFT,
            BOTH_CARTWHEEL_RIGHT,
        ];
        let mut special_attack = AnimSet([0; WORDS]);
        for m in moves().filter(|m| m.special()) {
            let anim = m.data().anim;
            if !side_moves.contains(&anim) {
                special_attack.add(anim);
            }
        }
        let mut kata = AnimSet([0; WORDS]);
        for m in moves().filter(|m| m.kata()) {
            kata.add(m.data().anim);
        }

        Families {
            running: starts("BOTH_RUN"),
            landing: starts("BOTH_LAND"),
            slope: starts("LEGS_"),
            knockdown: AnimSet::of(|a| a == BOTH_KNOCKDOWN1 || a.name().contains("GETUP")),
            kicking: starts("BOTH_A7_KICK_").union(AnimSet::named(&[
                "BOTH_A7_HILT",
                "BOTH_GETUP_BROLL_F",
                "BOTH_GETUP_BROLL_B",
            ])),
            kata,
            special_jump: flipping.union(AnimSet::named(&[
                "BOTH_FORCELEAP2_T__B_",
                "BOTH_FORCELONGLEAP_ATTACK",
            ])),
            flipping,
            jumping: AnimSet::named(&[
                "BOTH_INAIR1",
                "BOTH_JUMP1",
                "BOTH_JUMPBACK1",
                "BOTH_JUMPLEFT1",
                "BOTH_JUMPRIGHT1",
                "BOTH_LAND1",
                "BOTH_LANDBACK1",
            ]),
            special_attack,
            spinning: spinning_set(),
            single_transition: starts("BOTH_T1_")
                .union(starts("BOTH_T2_"))
                .union(starts("BOTH_T3_")),
        }
    }
}

/// The spinning animations: the transitions that swing the blade across
/// the body low enough for each style, and a few others.
fn spinning_set() -> AnimSet {
    // The ring's quadrants by side, with their height (bottom 0, middle 1,
    // top 2), as transition names spell them.
    const RIGHT: [(&str, u8); 3] = [("BR", 0), ("_R", 1), ("TR", 2)];
    const LEFT: [(&str, u8); 3] = [("BL", 0), ("_L", 1), ("TL", 2)];
    let transition = |style: u8, from: &str, to: &str| format!("BOTH_T{style}_{from}_{to}");
    // The crossings of a style (right to left and left to right) whose two
    // heights add up to `most` or less.
    let crossings = |style: u8, most: u8| {
        let mut names = Vec::new();
        for (a, b) in [(RIGHT, LEFT), (LEFT, RIGHT)] {
            for (from, high_from) in a {
                for (to, high_to) in b {
                    if high_from + high_to <= most {
                        names.push(transition(style, from, to));
                    }
                }
            }
        }
        names
    };
    let as_set =
        |names: Vec<String>| AnimSet::named(&names.iter().map(String::as_str).collect::<Vec<_>>());

    let fast = as_set(crossings(1, 2));
    let medium = as_set(crossings(2, 1));
    let strong = as_set(crossings(3, 1));
    let mut dual = crossings(6, 4);
    for (from, to) in [("T_", "TL"), ("T_", "BL"), ("_L", "T_"), ("BL", "T_")] {
        dual.push(transition(6, from, to));
    }
    let dual = as_set(dual);
    let staff = AnimSet::named(&[
        "BOTH_T7_BL_BR",
        "BOTH_T7_BL_TR",
        "BOTH_T7_BL__R",
        "BOTH_T7_BR_BL",
        "BOTH_T7_BR_TL",
        "BOTH_T7_BR__L",
        "BOTH_T7_TL_BR",
        "BOTH_T7_TL_TR",
        "BOTH_T7_TR__L",
        "BOTH_T7_T__BR",
        "BOTH_T7_T___R",
        "BOTH_T7__L_BR",
        "BOTH_T7__L_TR",
        "BOTH_T7__L__R",
        "BOTH_T7__R_BL",
        "BOTH_T7__R__L",
    ]);
    let others = AnimSet::named(&[
        "BOTH_ATTACK_BACK",
        "BOTH_BUTTERFLY_LEFT",
        "BOTH_BUTTERFLY_RIGHT",
        "BOTH_CROUCHATTACKBACK1",
        "BOTH_JUMPFLIPSLASHDOWN1",
        "BOTH_JUMPFLIPSTABDOWN",
        "BOTH_V7_BL_S7",
    ]);
    fast.union(medium)
        .union(strong)
        .union(dual)
        .union(staff)
        .union(others)
}

/// One of the four rolls (not the roll's stab).
pub(crate) fn is_roll(anim: Anim) -> bool {
    matches!(anim, BOTH_ROLL_B | BOTH_ROLL_F | BOTH_ROLL_L | BOTH_ROLL_R)
}

/// `anim` is a roll and the legs' timer still runs.
pub(crate) fn roll_playing(ps: &MotionState, anim: Anim) -> bool {
    is_roll(anim) && ps.legs.timer > 0
}

/// `anim` is a roll and the legs' timer has run out.
pub(crate) fn roll_spent(ps: &MotionState, anim: Anim) -> bool {
    is_roll(anim) && ps.legs.timer < 1
}

pub(crate) fn is_run(anim: Anim) -> bool {
    FAMILIES.running.has(anim)
}

pub(crate) fn is_landing(anim: Anim) -> bool {
    FAMILIES.landing.has(anim)
}

/// One of the slope stances.
pub(crate) fn is_slope_stance(anim: Anim) -> bool {
    FAMILIES.slope.has(anim)
}

/// Lying knocked down, or getting up.
pub(crate) fn is_knockdown(anim: Anim) -> bool {
    FAMILIES.knockdown.has(anim)
}

pub(crate) fn is_kick(anim: Anim) -> bool {
    FAMILIES.kicking.has(anim)
}

pub(crate) fn is_kata(anim: Anim) -> bool {
    FAMILIES.kata.has(anim)
}

/// A flip, or one of the two leaping attacks.
pub(crate) fn is_leap(anim: Anim) -> bool {
    FAMILIES.special_jump.has(anim)
}

pub(crate) fn is_flip(anim: Anim) -> bool {
    FAMILIES.flipping.has(anim)
}

pub(crate) fn is_jump(anim: Anim) -> bool {
    FAMILIES.jumping.has(anim)
}

/// The animation of a special attack, but the arials and cartwheels.
pub(crate) fn is_special_move(anim: Anim) -> bool {
    FAMILIES.special_attack.has(anim)
}

pub(crate) fn is_spin(anim: Anim) -> bool {
    FAMILIES.spinning.has(anim)
}

/// One of the lit saber stances.
pub(crate) fn is_lit_stance(anim: Anim) -> bool {
    matches!(
        anim,
        BOTH_STAND2
            | BOTH_SABERDUAL_STANCE
            | BOTH_SABERFAST_STANCE
            | BOTH_SABERSLOW_STANCE
            | BOTH_SABERSTAFF_STANCE
    )
}

pub(crate) fn standing(anim: Anim) -> bool {
    matches!(
        anim,
        BOTH_STAND1
            | BOTH_STAND1TO2
            | BOTH_STAND2
            | BOTH_STAND2TO1
            | TORSO_DROPWEAP1
            | TORSO_RAISEWEAP1
            | TORSO_WEAPONREADY2
            | TORSO_WEAPONREADY3
    )
}

/// A transition of one of the three single-blade styles.
pub(crate) fn single_transition(anim: Anim) -> bool {
    FAMILIES.single_transition.has(anim)
}
