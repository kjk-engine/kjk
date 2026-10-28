//! The saber move table: every move a blade can make, what it plays, where
//! the blade is when it starts and when it ends, how the animation holds,
//! what it blocks while it plays and what follows it; and the small maps
//! and the chain-angle rule over the table.
//!
//! Most of the table is regular. Seven swings cross the swinger's view
//! between the eight quadrants, and each one has an attack, a start that
//! raises the blade to it and a return that lowers it again. Between any
//! two quadrants of the ring there is a transition; at each quadrant a
//! bounce, a deflection and a broken attack; and five guards each have a
//! broken parry, a knockaway, a parry and a reflect. Those families are
//! generated here from their rules, with their few exceptions written out
//! where they fall. The 51 special attacks (lunges, flips, kicks, katas)
//! follow no rule and are listed as game facts.
//!
//! The table is built once, the first time it is asked. A move's number is
//! its place in the order below; snapshots, saves and the sim hash store
//! it, so the order never changes.

use std::sync::LazyLock;

use crate::motion::Anim;

use Move::*;

/// Where the blade is in the swinger's view: clockwise from bottom right,
/// with bottom last. The first seven are the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quad {
    BR,
    R,
    TR,
    T,
    TL,
    L,
    BL,
    B,
}

impl Quad {
    pub const ALL: [Quad; 8] = [
        Quad::BR,
        Quad::R,
        Quad::TR,
        Quad::T,
        Quad::TL,
        Quad::L,
        Quad::BL,
        Quad::B,
    ];

    /// The seven quadrants round the ring, bottom left out.
    const RING: [Quad; 7] = [
        Quad::BR,
        Quad::R,
        Quad::TR,
        Quad::T,
        Quad::TL,
        Quad::L,
        Quad::BL,
    ];

    fn n(self) -> usize {
        self as usize
    }

    /// The two letters the quadrant has in move and animation names: a
    /// one-letter quadrant is padded, before for the sides, after for top
    /// and bottom.
    fn code(self) -> &'static str {
        match self {
            Quad::BR => "BR",
            Quad::R => "_R",
            Quad::TR => "TR",
            Quad::T => "T_",
            Quad::TL => "TL",
            Quad::L => "_L",
            Quad::BL => "BL",
            Quad::B => "B_",
        }
    }

    /// Left for right; top and bottom stay where they are.
    fn mirror(self) -> Quad {
        match self {
            Quad::BR => Quad::BL,
            Quad::R => Quad::L,
            Quad::TR => Quad::TL,
            Quad::TL => Quad::TR,
            Quad::L => Quad::R,
            Quad::BL => Quad::BR,
            q => q,
        }
    }

    /// Across the view: four places on round all eight.
    fn opposite(self) -> Quad {
        Quad::ALL[(self.n() + 4) % 8]
    }

    fn upper_diagonal(self) -> bool {
        matches!(self, Quad::TR | Quad::TL)
    }

    fn lower_diagonal(self) -> bool {
        matches!(self, Quad::BR | Quad::BL)
    }
}

/// How a move's animation is set: over whatever plays and held a frame
/// short of its length, held its whole length, or only when nothing else
/// is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    Active,
    Finish,
    Idle,
}

/// What the blade blocks while a move plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Block {
    #[default]
    No = 0,
    Tight = 1,
    Wide = 2,
}

/// One row of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveData {
    pub anim: Anim,
    pub start: Quad,
    pub end: Quad,
    pub hold: Hold,
    pub block: Block,
    /// What follows when attack is not held as the move ends.
    pub chain_idle: Move,
    /// What follows when attack is held and nothing else is chosen.
    pub chain_attack: Move,
}

macro_rules! moves {
    ($($name:ident),* $(,)?) => {
        /// Every saber move, numbered by its place here (frozen: stored in
        /// snapshots and saves).
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        #[repr(u8)]
        pub enum Move {
            #[default]
            $($name),*
        }

        impl Move {
            /// Every move, in number order.
            pub const ALL: &'static [Move] = &[$(Move::$name),*];

            pub fn name(self) -> &'static str {
                match self {
                    $(Move::$name => stringify!($name)),*
                }
            }
        }
    };
}

moves!(
    // Standing, drawing and sheathing.
    NONE,
    READY,
    DRAW,
    PUTAWAY,
    // The attacks of the seven swings.
    A_TL2BR,
    A_L2R,
    A_BL2TR,
    A_BR2TL,
    A_R2L,
    A_TR2BL,
    A_T2B,
    // The special attacks.
    A_BACKSTAB,
    A_BACK,
    A_BACK_CR,
    ROLL_STAB,
    A_LUNGE,
    A_JUMP_T__B_,
    A_FLIP_STAB,
    A_FLIP_SLASH,
    JUMPATTACK_DUAL,
    JUMPATTACK_ARIAL_LEFT,
    JUMPATTACK_ARIAL_RIGHT,
    JUMPATTACK_CART_LEFT,
    JUMPATTACK_CART_RIGHT,
    JUMPATTACK_STAFF_LEFT,
    JUMPATTACK_STAFF_RIGHT,
    BUTTERFLY_LEFT,
    BUTTERFLY_RIGHT,
    A_BACKFLIP_ATK,
    SPINATTACK_DUAL,
    SPINATTACK,
    LEAP_ATTACK,
    SWOOP_ATTACK_RIGHT,
    SWOOP_ATTACK_LEFT,
    TAUNTAUN_ATTACK_RIGHT,
    TAUNTAUN_ATTACK_LEFT,
    KICK_F,
    KICK_B,
    KICK_R,
    KICK_L,
    KICK_S,
    KICK_BF,
    KICK_RL,
    KICK_F_AIR,
    KICK_B_AIR,
    KICK_R_AIR,
    KICK_L_AIR,
    STABDOWN,
    STABDOWN_STAFF,
    STABDOWN_DUAL,
    DUAL_SPIN_PROTECT,
    STAFF_SOULCAL,
    A1_SPECIAL,
    A2_SPECIAL,
    A3_SPECIAL,
    UPSIDE_DOWN_ATTACK,
    PULL_ATTACK_STAB,
    PULL_ATTACK_SWING,
    SPINATTACK_ALORA,
    DUAL_FB,
    DUAL_LR,
    HILT_BASH,
    // Raising the blade into each swing.
    S_TL2BR,
    S_L2R,
    S_BL2TR,
    S_BR2TL,
    S_R2L,
    S_TR2BL,
    S_T2B,
    // Lowering it after each swing.
    R_TL2BR,
    R_L2R,
    R_BL2TR,
    R_BR2TL,
    R_R2L,
    R_TR2BL,
    R_T2B,
    // From each ring quadrant to each other one.
    T1_BR__R,
    T1_BR_TR,
    T1_BR_T_,
    T1_BR_TL,
    T1_BR__L,
    T1_BR_BL,
    T1__R_BR,
    T1__R_TR,
    T1__R_T_,
    T1__R_TL,
    T1__R__L,
    T1__R_BL,
    T1_TR_BR,
    T1_TR__R,
    T1_TR_T_,
    T1_TR_TL,
    T1_TR__L,
    T1_TR_BL,
    T1_T__BR,
    T1_T___R,
    T1_T__TR,
    T1_T__TL,
    T1_T___L,
    T1_T__BL,
    T1_TL_BR,
    T1_TL__R,
    T1_TL_TR,
    T1_TL_T_,
    T1_TL__L,
    T1_TL_BL,
    T1__L_BR,
    T1__L__R,
    T1__L_TR,
    T1__L_T_,
    T1__L_TL,
    T1__L_BL,
    T1_BL_BR,
    T1_BL__R,
    T1_BL_TR,
    T1_BL_T_,
    T1_BL_TL,
    T1_BL__L,
    // Bounces round the ring.
    B1_BR,
    B1__R,
    B1_TR,
    B1_T_,
    B1_TL,
    B1__L,
    B1_BL,
    // Deflections at all eight.
    D1_BR,
    D1__R,
    D1_TR,
    D1_T_,
    D1_TL,
    D1__L,
    D1_BL,
    D1_B_,
    // Broken attacks at all eight.
    V1_BR,
    V1__R,
    V1_TR,
    V1_T_,
    V1_TL,
    V1__L,
    V1_BL,
    V1_B_,
    // Broken parries.
    H1_T_,
    H1_TR,
    H1_TL,
    H1_BR,
    H1_B_,
    H1_BL,
    // The guards' knockaways, parries and reflects.
    K1_T_,
    K1_TR,
    K1_TL,
    K1_BR,
    K1_BL,
    PARRY_UP,
    PARRY_UR,
    PARRY_UL,
    PARRY_LR,
    PARRY_LL,
    REFLECT_UP,
    REFLECT_UR,
    REFLECT_UL,
    REFLECT_LR,
    REFLECT_LL,
);

/// Where each family begins in the numbering.
const ATTACKS: usize = A_TL2BR as usize;
const SPECIALS: usize = A_BACKSTAB as usize;
const KICKS: usize = KICK_F as usize;
const AFTER_KICKS: usize = STABDOWN as usize;
const STARTS: usize = S_TL2BR as usize;
const RETURNS: usize = R_TL2BR as usize;
const TRANSITIONS: usize = T1_BR__R as usize;
const BOUNCES: usize = B1_BR as usize;
const BROKEN_ATTACKS: usize = V1_BR as usize;
const BROKEN_PARRIES: usize = H1_T_ as usize;
const KNOCKAWAYS: usize = K1_T_ as usize;
const PARRIES: usize = PARRY_UP as usize;
const REFLECTS: usize = REFLECT_UP as usize;

/// The seven swings, from quadrant to quadrant, in their fixed order.
const SWINGS: [(Quad, Quad); 7] = [
    (Quad::TL, Quad::BR),
    (Quad::L, Quad::R),
    (Quad::BL, Quad::TR),
    (Quad::BR, Quad::TL),
    (Quad::R, Quad::L),
    (Quad::TR, Quad::BL),
    (Quad::T, Quad::B),
];

/// The five guards, in the order their knockaways, parries and reflects
/// come.
const GUARDS: [Quad; 5] = [Quad::T, Quad::TR, Quad::TL, Quad::BR, Quad::BL];

/// The broken parries' quadrants: the guards with the bottom before BL.
const BROKEN_PARRY_AT: [Quad; 6] = [Quad::T, Quad::TR, Quad::TL, Quad::BR, Quad::B, Quad::BL];

fn nth(index: usize) -> Move {
    Move::ALL[index]
}

/// The swing that starts at `q` (every ring quadrant starts one).
fn swing_from(q: Quad) -> Option<usize> {
    SWINGS.iter().position(|&(from, _)| from == q)
}

/// The swing that ends at `q` (none ends at the top).
fn swing_to(q: Quad) -> Option<usize> {
    SWINGS.iter().position(|&(_, to)| to == q)
}

/// The plain attack of the swing starting at a ring quadrant.
fn attack_from(q: Quad) -> Move {
    let swing = swing_from(q).expect("every ring quadrant starts a swing");
    nth(ATTACKS + swing)
}

/// The return of the swing ending at `q`; the top, where none ends, takes
/// the top right's.
fn return_at(q: Quad) -> Move {
    match swing_to(q) {
        Some(swing) => nth(RETURNS + swing),
        None => return_at(Quad::TR),
    }
}

/// The transition between two different ring quadrants.
fn ring_transition(from: Quad, to: Quad) -> Move {
    let (f, t) = (from.n(), to.n());
    debug_assert!(f != t && f < 7 && t < 7);
    nth(TRANSITIONS + 6 * f + t - usize::from(t > f))
}

/// Where a bounce at a ring quadrant sends the blade next (game facts).
fn rebound(q: Quad) -> Quad {
    match q {
        Quad::BR => Quad::TR,
        Quad::R => Quad::L,
        Quad::TR => Quad::TL,
        Quad::T => Quad::BL,
        Quad::TL => Quad::TR,
        Quad::L => Quad::R,
        Quad::BL => Quad::TR,
        Quad::B => unreachable!("no bounce at the bottom"),
    }
}

/// Where a knockaway ending at `q` carries on to (game facts).
fn knockaway_follow(q: Quad) -> Quad {
    match q {
        Quad::T => Quad::BR,
        Quad::TR => Quad::R,
        Quad::TL => Quad::L,
        Quad::BL => Quad::TL,
        Quad::BR => Quad::TR,
        _ => unreachable!("knockaways end only at a guard"),
    }
}

/// The word a guard has in parry and reflect names.
fn guard_word(q: Quad) -> &'static str {
    match q {
        Quad::T => "UP",
        Quad::TR => "UR",
        Quad::TL => "UL",
        Quad::BR => "LR",
        Quad::BL => "LL",
        _ => unreachable!("not a guard"),
    }
}

fn swing_word((from, to): (Quad, Quad)) -> String {
    format!("{from:?}2{to:?}")
}

/// A special attack's row as it differs from the specials' default: from
/// right to right, tight, back to ready either way.
#[derive(Clone, Copy)]
struct Special {
    anim: Anim,
    start: Quad,
    end: Quad,
    block: Block,
    attack: Move,
}

const fn special(anim: Anim) -> Special {
    Special {
        anim,
        start: Quad::R,
        end: Quad::R,
        block: Block::Tight,
        attack: READY,
    }
}

impl Special {
    const fn path(self, start: Quad, end: Quad) -> Special {
        Special { start, end, ..self }
    }

    const fn then(self, attack: Move) -> Special {
        Special { attack, ..self }
    }

    fn row(self) -> MoveData {
        MoveData {
            anim: self.anim,
            start: self.start,
            end: self.end,
            hold: Hold::Active,
            block: self.block,
            chain_idle: READY,
            chain_attack: self.attack,
        }
    }
}

/// The specials before the kicks (game facts).
const LEADING_SPECIALS: [Special; KICKS - SPECIALS] = {
    use Anim::*;
    use Quad::*;
    [
        special(BOTH_A2_STABBACK1),
        special(BOTH_ATTACK_BACK),
        special(BOTH_CROUCHATTACKBACK1),
        special(BOTH_ROLL_STAB),
        special(BOTH_LUNGE2_B__T_).path(B, T),
        special(BOTH_FORCELEAP2_T__B_).path(T, B),
        special(BOTH_JUMPFLIPSTABDOWN).path(R, T).then(T1_T___R),
        special(BOTH_JUMPFLIPSLASHDOWN1).path(L, R).then(T1__R_T_),
        special(BOTH_JUMPATTACK6).path(R, BL).then(T1_BL_TR),
        special(BOTH_ARIAL_LEFT).path(R, TL).then(A_TL2BR),
        special(BOTH_ARIAL_RIGHT).path(R, TR).then(A_TR2BL),
        special(BOTH_CARTWHEEL_LEFT).path(R, TL).then(T1_TL_BR),
        special(BOTH_CARTWHEEL_RIGHT).path(R, TR).then(T1_TR_BL),
        special(BOTH_BUTTERFLY_FL1).path(R, L).then(T1__L__R),
        special(BOTH_BUTTERFLY_FR1).then(T1__R__L),
        special(BOTH_BUTTERFLY_LEFT).path(R, L).then(T1__L__R),
        special(BOTH_BUTTERFLY_RIGHT).then(T1__R__L),
        special(BOTH_JUMPATTACK7).path(B, T).then(T1_T___R),
        special(BOTH_SPINATTACK6),
        special(BOTH_SPINATTACK7).path(L, R),
        special(BOTH_FORCELONGLEAP_ATTACK).path(R, L),
        special(BOTH_VS_ATR_S).path(R, T),
        special(BOTH_VS_ATL_S).path(L, T),
        special(BOTH_VT_ATR_S).path(R, T),
        special(BOTH_VT_ATL_S).path(L, T),
    ]
};

/// The specials after the kicks (game facts).
const TRAILING_SPECIALS: [Special; STARTS - AFTER_KICKS] = {
    use Anim::*;
    [
        special(BOTH_STABDOWN).then(S_R2L),
        special(BOTH_STABDOWN_STAFF).then(S_R2L),
        special(BOTH_STABDOWN_DUAL).then(S_R2L),
        special(BOTH_A6_SABERPROTECT),
        special(BOTH_A7_SOULCAL),
        special(BOTH_A1_SPECIAL),
        special(BOTH_A2_SPECIAL),
        special(BOTH_A3_SPECIAL),
        special(BOTH_FLIP_ATTACK7),
        special(BOTH_PULL_IMPALE_STAB),
        special(BOTH_PULL_IMPALE_SWING),
        special(BOTH_ALORA_SPIN_SLASH),
        special(BOTH_A6_FB),
        special(BOTH_A6_LR),
        special(BOTH_A7_HILT),
    ]
};

/// The animation of that name; every name the rules make is one.
fn anim(name: &str) -> Anim {
    Anim::from_name(name).unwrap_or_else(|| panic!("no animation {name}"))
}

/// The table, built from the families' rules in number order.
struct Builder {
    rows: Vec<MoveData>,
}

impl Builder {
    /// Adds the next move's row, checking in debug builds that the row's
    /// name, as the rules make it, is the move's.
    fn push(&mut self, name: &str, row: MoveData) {
        debug_assert_eq!(nth(self.rows.len()).name(), name);
        self.rows.push(row);
    }

    fn build() -> Vec<MoveData> {
        let mut b = Builder {
            rows: Vec::with_capacity(Move::ALL.len()),
        };
        let row = |anim, start, end, hold, block, chain_idle, chain_attack| MoveData {
            anim,
            start,
            end,
            hold,
            block,
            chain_idle,
            chain_attack,
        };
        let r = Quad::R;

        // Standing, drawing and sheathing.
        let stand1 = Anim::BOTH_STAND1;
        b.push("NONE", row(stand1, r, r, Hold::Idle, Block::No, NONE, NONE));
        let stand2 = Anim::BOTH_STAND2;
        b.push(
            "READY",
            row(stand2, r, r, Hold::Idle, Block::Wide, READY, S_R2L),
        );
        let draw = Anim::BOTH_STAND1TO2;
        b.push(
            "DRAW",
            row(draw, r, r, Hold::Finish, Block::No, READY, S_R2L),
        );
        let put = Anim::BOTH_STAND2TO1;
        b.push(
            "PUTAWAY",
            row(put, r, r, Hold::Finish, Block::No, READY, S_R2L),
        );

        // The attacks: the swing itself, then its return.
        for (i, swing @ (from, to)) in SWINGS.into_iter().enumerate() {
            let ret = nth(RETURNS + i);
            let a = anim(&format!("BOTH_A1_{}_{}", from.code(), to.code()));
            let data = row(a, from, to, Hold::Active, Block::Tight, ret, ret);
            b.push(&format!("A_{}", swing_word(swing)), data);
        }

        // The specials, the kicks named after their animations.
        for s in LEADING_SPECIALS {
            b.push(nth(b.rows.len()).name(), s.row());
        }
        for kick in &Move::ALL[KICKS..AFTER_KICKS] {
            let mut s = special(anim(&format!("BOTH_A7_{}", kick.name()))).then(S_R2L);
            if matches!(kick, KICK_S | KICK_BF | KICK_RL) {
                s.block = Block::No;
            }
            b.push(kick.name(), s.row());
        }
        for s in TRAILING_SPECIALS {
            b.push(nth(b.rows.len()).name(), s.row());
        }

        // Starts raise the blade from the right into a swing.
        for (i, swing @ (from, _)) in SWINGS.into_iter().enumerate() {
            let attack = nth(ATTACKS + i);
            let a = anim(&format!("BOTH_S1_S1_{}", from.code()));
            let data = row(a, r, from, Hold::Active, Block::Tight, attack, attack);
            b.push(&format!("S_{}", swing_word(swing)), data);
        }

        // Returns bring it back to the right after one.
        for swing @ (_, to) in SWINGS {
            let a = anim(&format!("BOTH_R1_{}_S1", to.code()));
            let data = row(a, to, r, Hold::Finish, Block::Tight, READY, READY);
            b.push(&format!("R_{}", swing_word(swing)), data);
        }

        // Transitions: idle back from where they end, or attack from there.
        for from in Quad::RING {
            for to in Quad::RING.into_iter().filter(|&to| to != from) {
                let codes = format!("{}_{}", from.code(), to.code());
                let data = row(
                    anim(&format!("BOTH_T1_{codes}")),
                    from,
                    to,
                    Hold::Active,
                    Block::No,
                    return_at(to),
                    attack_from(to),
                );
                b.push(&format!("T1_{codes}"), data);
            }
        }

        // A bounce stays where it is, then returns or rebounds. A
        // deflection is the same move under its own animation, except at
        // the top, which plays the bounce's, and at the bottom, where
        // there is no bounce: it stays at the bottom and carries on as the
        // top does.
        let bounce = |q: Quad, a: Anim| {
            let links = if q == Quad::B { Quad::T } else { q };
            let next = ring_transition(links, rebound(links));
            row(a, q, q, Hold::Active, Block::No, return_at(links), next)
        };
        for q in Quad::RING {
            let a = anim(&format!("BOTH_B1_{}___", q.code()));
            b.push(&format!("B1_{}", q.code()), bounce(q, a));
        }
        for q in Quad::ALL {
            let kind = if q == Quad::T { "B1" } else { "D1" };
            let a = anim(&format!("BOTH_{kind}_{}___", q.code()));
            b.push(&format!("D1_{}", q.code()), bounce(q, a));
        }

        // Broken attacks stay put and go back to ready.
        for q in Quad::ALL {
            let a = anim(&format!("BOTH_V1_{}_S1", q.code()));
            let data = row(a, q, q, Hold::Active, Block::No, READY, READY);
            b.push(&format!("V1_{}", q.code()), data);
        }

        // Broken parries are knocked across the view; the lower diagonals
        // start on the other side.
        for q in BROKEN_PARRY_AT {
            let start = if q.lower_diagonal() { q.mirror() } else { q };
            let a = anim(&format!("BOTH_H1_S1_{}", q.code()));
            let end = start.opposite();
            let data = row(a, start, end, Hold::Active, Block::No, READY, READY);
            b.push(&format!("H1_{}", q.code()), data);
        }

        // The guards all start from the right and block wide.
        let guard = |a, end, chain_idle, chain_attack| {
            row(
                a,
                r,
                end,
                Hold::Active,
                Block::Wide,
                chain_idle,
                chain_attack,
            )
        };
        // Knockaways: the lower diagonals end mirrored, and carry on from
        // where they end.
        for q in GUARDS {
            let end = if q.lower_diagonal() { q.mirror() } else { q };
            let a = anim(&format!("BOTH_K1_S1_{}", q.code()));
            let next = ring_transition(end, knockaway_follow(end));
            b.push(
                &format!("K1_{}", q.code()),
                guard(a, end, return_at(q), next),
            );
        }
        // Parries: the upper diagonals end mirrored; the links go by the
        // guard itself.
        for q in GUARDS {
            let end = if q.upper_diagonal() { q.mirror() } else { q };
            let a = anim(&format!("BOTH_P1_S1_{}", q.code()));
            let data = guard(a, end, return_at(q), attack_from(q));
            b.push(&format!("PARRY_{}", guard_word(q)), data);
        }
        // Reflects: links by the mirrored guard; the upper diagonals play
        // the mirrored parry's animation, the lower ones end mirrored.
        for q in GUARDS {
            let m = q.mirror();
            let shown = if q.upper_diagonal() { m } else { q };
            let end = if q.lower_diagonal() { m } else { q };
            let a = anim(&format!("BOTH_P1_S1_{}", shown.code()));
            let data = guard(a, end, return_at(m), attack_from(m));
            b.push(&format!("REFLECT_{}", guard_word(q)), data);
        }

        debug_assert_eq!(b.rows.len(), Move::ALL.len());
        b.rows
    }
}

static TABLE: LazyLock<Vec<MoveData>> = LazyLock::new(Builder::build);

impl Move {
    pub fn index(self) -> u16 {
        self as u16
    }

    pub fn from_index(index: u16) -> Option<Move> {
        Move::ALL.get(usize::from(index)).copied()
    }

    pub fn data(self) -> &'static MoveData {
        &TABLE[self as usize]
    }

    fn within(self, first: usize, end: usize) -> bool {
        (first..end).contains(&(self as usize))
    }

    /// One of the seven swings' attacks.
    pub fn pure_attack(self) -> bool {
        self.within(ATTACKS, SPECIALS)
    }

    /// A swing's attack or a special.
    pub fn attack(self) -> bool {
        self.within(ATTACKS, STARTS)
    }

    pub fn special(self) -> bool {
        self.within(SPECIALS, STARTS)
    }

    pub fn kata(self) -> bool {
        self.within(DUAL_SPIN_PROTECT as usize, A3_SPECIAL as usize + 1)
    }

    pub fn kick(self) -> bool {
        self.within(KICKS, AFTER_KICKS) || self == HILT_BASH
    }

    pub fn idle(self) -> bool {
        self.within(0, ATTACKS)
    }

    pub fn start(self) -> bool {
        self.within(STARTS, RETURNS)
    }

    pub fn ret(self) -> bool {
        self.within(RETURNS, TRANSITIONS)
    }

    pub fn transition(self) -> bool {
        self.within(TRANSITIONS, BOUNCES)
    }

    /// A start, a return or a transition.
    pub fn in_between(self) -> bool {
        self.within(STARTS, BOUNCES)
    }

    /// A bounce or a deflection, but not the bottom deflection (a game
    /// fact: it never counted as a bounce).
    pub fn bounce(self) -> bool {
        self.within(BOUNCES, D1_B_ as usize)
    }

    /// A broken attack or a broken parry.
    pub fn broken(self) -> bool {
        self.within(BROKEN_ATTACKS, KNOCKAWAYS)
    }

    pub fn knockaway(self) -> bool {
        self.within(KNOCKAWAYS, PARRIES)
    }

    pub fn parry(self) -> bool {
        self.within(PARRIES, REFLECTS)
    }

    pub fn reflect(self) -> bool {
        self.within(REFLECTS, Move::ALL.len())
    }

    /// The attack a start leads into. Only for starts.
    pub fn attack_of_start(self) -> Move {
        debug_assert!(self.start(), "{} is not a start", self.name());
        nth(self as usize - STARTS + ATTACKS)
    }

    /// The start of a swing's attack. Only for the seven attacks.
    pub fn start_of_attack(self) -> Move {
        debug_assert!(self.pure_attack(), "{} is not a swing", self.name());
        nth(self as usize - ATTACKS + STARTS)
    }

    /// The return after a swing's attack. Only for the seven attacks.
    pub fn return_of_attack(self) -> Move {
        debug_assert!(self.pure_attack(), "{} is not a swing", self.name());
        nth(self as usize - ATTACKS + RETURNS)
    }
}

/// The move that takes the blade from one quadrant to another: none to
/// where it already is or to the bottom. From the bottom it moves as from
/// the bottom right, except to the bottom right itself, which it reaches
/// as from the bottom left.
pub fn transition(from: Quad, to: Quad) -> Option<Move> {
    if from == to || to == Quad::B {
        return None;
    }
    let from = match (from, to) {
        (Quad::B, Quad::BR) => Quad::BL,
        (Quad::B, _) => Quad::BR,
        _ => from,
    };
    Some(ring_transition(from, to))
}

/// The angle in degrees from where `from` ends to where `to` starts. Round
/// the ring it never passes the bottom; with the bottom it takes the short
/// way round all eight. Five steps apart is 215, not 225: the game's own
/// figure, kept.
pub fn chain_angle(from: Move, to: Move) -> u16 {
    let (a, b) = (from.data().end, to.data().start);
    let apart = a.n().abs_diff(b.n());
    let steps = if a == Quad::B || b == Quad::B {
        apart.min(8 - apart)
    } else {
        apart
    };
    match steps {
        5 => 215,
        steps => 45 * steps as u16,
    }
}

/// The attack a parry at a guard can turn into; none at the sides or the
/// bottom.
pub fn attack_from_parry(quad: Quad) -> Option<Move> {
    GUARDS.contains(&quad).then(|| attack_from(quad))
}

/// The bounce for a move hitting something: at the quadrant it starts
/// from, the bottom right for one starting at the bottom.
pub fn bounce_for(attack: Move) -> Move {
    let start = attack.data().start;
    let at = if start == Quad::B { Quad::BR } else { start };
    nth(BOUNCES + at.n())
}

/// The broken parry a parry (or standing ready) turns into when it is
/// beaten; the lower guards break to the other side.
pub fn broken_parry_for(parry: Move) -> Option<Move> {
    let at = match parry {
        PARRY_UP => Quad::T,
        PARRY_UR => Quad::TR,
        PARRY_UL => Quad::TL,
        PARRY_LR => Quad::BL,
        PARRY_LL => Quad::BR,
        READY => Quad::B,
        _ => return None,
    };
    let place = BROKEN_PARRY_AT.iter().position(|&q| q == at)?;
    Some(nth(BROKEN_PARRIES + place))
}

/// The return that brings the blade back from `quad`.
pub fn return_from(quad: Quad) -> Move {
    return_at(quad)
}
