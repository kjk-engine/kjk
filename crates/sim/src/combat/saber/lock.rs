//! Saber locks: two blades bound together until one side forces its way
//! through.
//!
//! Two fighters close enough, level, on the ground and facing each other
//! may lock when their blades meet (`can_lock`); their torsos' swings, or
//! a defender's wide block or parry, pick the kind of lock, or now and
//! then a random one (`pick`). Two single sabers play the old locks; the
//! pair or the staff on either side plays the new ones, chosen by each
//! side's class and whether the lock is high or to the side
//! (`opening`). Each side's frame is held on its lock animation (`Lock`);
//! a press banks hits (`press_hits`) and each hit spent moves the frame
//! toward that side's winning end (`advance`), the other side's frame
//! mirroring it. The side that reaches its end wins: the two play their
//! breaks (`breaks`), sometimes a super-break that leaves the loser
//! defenceless (`super_breaks`); a loser may be floored or killed outright
//! (`loss_kills`), and the two are shoved apart (`shove`).

use crate::math::{self, Vec3};
use crate::motion::{Anim, AnimTable, MotionState};
use crate::rng::SimRng;
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer};

use super::clash::{attack_anim, roll_between};
use super::{Block, Move, Style, power};
use Anim::*;
use rules::saber_locks::contest;
use rules::saber_locks::end;
use rules::saber_locks::start;

/// How long a lock lasts at most (ms).
pub const LOCK_MS: u64 = contest::LOCK_MS;
/// How long the loser of a lock with hits left lies (ms), and the shoves.
pub const LOSS_DOWN_MS: u64 = end::LOSS_DOWN_MS;
const LOSS_PUSH: f32 = end::LOSS_PUSH;
const LOSS_LIFT: f32 = end::LOSS_LIFT;
const APART_PUSH: f32 = end::APART_PUSH;
const APART_LIFT: f32 = end::APART_LIFT;
/// A banked hit is spent at most once per JA server frame (ms).
pub const ADVANCE_MS: u64 = power::JA_FRAME_MS as u64;
/// The distance a lock keeps (units), compared squared.
const NEAR: f32 = start::NEAR;
const FAR: f32 = start::FAR;

/// One side of a lock (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lock {
    pub enemy: SimId,
    /// Its lock animation, on the whole body, and the frame it is frozen on.
    pub anim: Anim,
    pub frame: i32,
    /// Hits banked by presses, and whether one is being spent.
    pub hits: u8,
    pub advance: bool,
    /// Attack was held on the last tick (a press is its edge).
    pub pressed: bool,
    /// The last JA server frame's check (sim ms): one per frame.
    pub checked_ms: u64,
}

impl Lock {
    pub(crate) const BLANK: Lock = Lock {
        enemy: SimId(0),
        anim: BOTH_STAND1,
        frame: 0,
        hits: 0,
        advance: false,
        pressed: false,
        checked_ms: 0,
    };

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.enemy.0);
        out.u16(self.anim.index());
        out.u32(self.frame as u32);
        out.u8(self.hits);
        out.u8(u8::from(self.advance));
        out.u8(u8::from(self.pressed));
        out.u64(self.checked_ms);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Lock, DecodeError> {
        let enemy = SimId(input.u32()?);
        let at = input.at();
        let anim = Anim::from_index(input.u16()?).ok_or(input.invalid("lock animation", at))?;
        let frame = input.u32()? as i32;
        let hits = input.u8()?;
        let mut flag = |what| {
            let at = input.at();
            match input.u8()? {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err(input.invalid(what, at)),
            }
        };
        let advance = flag("lock advance flag")?;
        let pressed = flag("lock press flag")?;
        Ok(Lock {
            enemy,
            anim,
            frame,
            hits,
            advance,
            pressed,
            checked_ms: input.u64()?,
        })
    }
}

/// The single pair's normal breaks, which no lock plays (two single
/// sabers lock the old way).
pub const NEVER_PLAYED: [Anim; 4] = [
    BOTH_LK_S_S_S_B_1_L,
    BOTH_LK_S_S_S_B_1_W,
    BOTH_LK_S_S_T_B_1_L,
    BOTH_LK_S_S_T_B_1_W,
];

/// The kind of lock: high, on one of the four diagonals, or to a side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Top,
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
    R,
    L,
}

/// Every kind, in the order a random lock picks from.
const KINDS: [Kind; 7] = [
    Kind::Top,
    Kind::TopRight,
    Kind::TopLeft,
    Kind::BottomRight,
    Kind::BottomLeft,
    Kind::R,
    Kind::L,
];

impl Kind {
    /// The kind's row in the start fractions: high, low diagonal, side.
    fn row(self) -> usize {
        match self {
            Kind::Top | Kind::TopRight | Kind::TopLeft => 0,
            Kind::BottomRight | Kind::BottomLeft => 1,
            Kind::R | Kind::L => 2,
        }
    }

    /// Whether, between two of a class, the attacker plays the second
    /// variant (else the defender does).
    fn attacker_second(self) -> bool {
        matches!(self, Kind::TopLeft | Kind::BottomLeft | Kind::R)
    }
}

/// A fighter as the lock's checks read it.
#[derive(Debug, Clone, Copy)]
pub struct Stance<'a> {
    pub ps: &'a MotionState,
    /// A player (JA's clients: their wide block with the weapon out locks
    /// any diagonal or side attack).
    pub player: bool,
    pub blocking: Block,
    pub weapon_wait_ms: i32,
    /// Knocked down, rolling, in a special jump, or a hand held out.
    pub busy: bool,
}

/// Whether two may lock: level enough, both on the ground and free,
/// neither ducked, the right distance apart, and each facing the other.
pub fn can_lock(a: &Stance, b: &Stance) -> bool {
    let (pa, pb) = (a.ps, b.ps);
    // A height that is not a number does not stop the lock.
    let height = (pa.origin[2] - pb.origin[2]).abs();
    (height.is_nan() || height <= start::HEIGHT)
        && pa.ground.is_some()
        && pb.ground.is_some()
        && !a.busy
        && !b.busy
        && !pa.ducked()
        && !pb.ducked()
        && in_reach(pa.origin, pb.origin)
        && faces(pa, pb.origin)
        && faces(pb, pa.origin)
}

/// The one standing at `ps` faces the point `toward`, seen from above.
fn faces(ps: &MotionState, toward: Vec3) -> bool {
    let (forward, _, _) = math::angle_vectors([0.0, ps.view[1], 0.0]);
    let mut way = math::sub(toward, ps.origin);
    way[2] = 0.0;
    math::normalize(&mut way);
    math::dot(way, forward) > start::FACING
}

/// The two origins are neither too close nor too far for a lock.
pub fn in_reach(a: Vec3, b: Vec3) -> bool {
    let apart = math::sub(a, b);
    (NEAR * NEAR..=FAR * FAR).contains(&math::dot(apart, apart))
}

/// What the other side must be doing for one of the swing rules to lock.
struct Answer {
    /// It swings this attack (in any style group)...
    swings: Option<Anim>,
    /// ...or plays this parry.
    plays: Anim,
    kind: Kind,
}

/// A swing that locks against the right reply: a wide block, or one of
/// two swings or parries, each giving its kind.
struct SwingRule {
    attack: Anim,
    wide: Kind,
    replies: [Answer; 2],
}

/// The swing rules, tried in this order.
const SWING_RULES: [SwingRule; 4] = [
    SwingRule {
        attack: BOTH_A1_TR_BL,
        wide: Kind::TopRight,
        replies: [
            Answer {
                swings: Some(BOTH_A1_TR_BL),
                plays: BOTH_P1_S1_TL,
                kind: Kind::TopRight,
            },
            Answer {
                swings: Some(BOTH_A1_BR_TL),
                plays: BOTH_P1_S1_BL,
                kind: Kind::BottomLeft,
            },
        ],
    },
    SwingRule {
        attack: BOTH_A1_TL_BR,
        wide: Kind::TopLeft,
        replies: [
            Answer {
                swings: Some(BOTH_A1_TL_BR),
                plays: BOTH_P1_S1_TR,
                kind: Kind::TopLeft,
            },
            Answer {
                swings: Some(BOTH_A1_BL_TR),
                plays: BOTH_P1_S1_BR,
                kind: Kind::BottomRight,
            },
        ],
    },
    SwingRule {
        attack: BOTH_A1__L__R,
        wide: Kind::L,
        replies: [
            Answer {
                swings: Some(BOTH_A1_TL_BR),
                plays: BOTH_P1_S1_TR,
                kind: Kind::L,
            },
            Answer {
                swings: None,
                plays: BOTH_P1_S1_BL,
                kind: Kind::L,
            },
        ],
    },
    SwingRule {
        attack: BOTH_A1__R__L,
        wide: Kind::R,
        replies: [
            Answer {
                swings: Some(BOTH_A1_TR_BL),
                plays: BOTH_P1_S1_TL,
                kind: Kind::R,
            },
            Answer {
                swings: None,
                plays: BOTH_P1_S1_BR,
                kind: Kind::R,
            },
        ],
    },
];

impl SwingRule {
    /// The kind this rule locks in against `them`, if any.
    fn against(&self, them: &Stance) -> Option<Kind> {
        let torso = them.ps.torso.anim;
        let wide = them.player && them.blocking == Block::Wide && them.weapon_wait_ms <= 0;
        if wide {
            return Some(self.wide);
        }
        self.replies
            .iter()
            .find(|reply| {
                reply
                    .swings
                    .is_some_and(|attack| attack_anim(torso, attack))
                    || torso == reply.plays
            })
            .map(|reply| reply.kind)
    }
}

/// Which lock the two fall into, and whether `a` is its attacker: a high
/// swing locks high; else the first swing rule a side's torso swings
/// decides it, for good or ill; else a lock now and then at random.
pub fn pick(a: &Stance, b: &Stance, rng: &mut SimRng) -> Option<(bool, Kind)> {
    let sides = [(true, a, b), (false, b, a)];
    for (a_attacks, me, _) in sides {
        if attack_anim(me.ps.torso.anim, BOTH_A1_T__B_) {
            return Some((a_attacks, Kind::Top));
        }
    }
    for rule in &SWING_RULES {
        for (a_attacks, me, them) in sides {
            if attack_anim(me.ps.torso.anim, rule.attack) {
                return rule.against(them).map(|kind| (a_attacks, kind));
            }
        }
    }
    if roll_between(rng, 0, start::RANDOM_ONE_IN - 1) != 0 {
        return None;
    }
    let last = KINDS.len() as i32 - 1;
    let kind = KINDS[roll_between(rng, 0, last) as usize];
    Some((true, kind))
}

/// The lock animations whose frame climbs toward their winning end (game
/// facts; every other lock's falls).
const RISING: [Anim; 16] = [
    BOTH_CWCIRCLELOCK,
    BOTH_BF1LOCK,
    BOTH_LK_DL_DL_S_L_1,
    BOTH_LK_DL_DL_S_L_2,
    BOTH_LK_DL_DL_T_L_1,
    BOTH_LK_DL_DL_T_L_2,
    BOTH_LK_DL_S_S_L_1,
    BOTH_LK_DL_S_T_L_1,
    BOTH_LK_DL_ST_S_L_1,
    BOTH_LK_DL_ST_T_L_1,
    BOTH_LK_S_S_S_L_1,
    BOTH_LK_S_S_T_L_2,
    BOTH_LK_ST_S_S_L_1,
    BOTH_LK_ST_S_T_L_1,
    BOTH_LK_ST_ST_T_L_1,
    BOTH_LK_ST_ST_T_L_2,
];

/// The frame of this lock animation climbs toward its winning end.
pub fn rises_to_win(anim: Anim) -> bool {
    RISING.contains(&anim)
}

/// The four locks two single sabers play.
fn old_lock(anim: Anim) -> bool {
    matches!(
        anim,
        BOTH_BF2LOCK | BOTH_BF1LOCK | BOTH_CWCIRCLELOCK | BOTH_CCWCIRCLELOCK
    )
}

/// A lock animation: an old lock, a new lock or a new lock's second
/// variant.
pub fn in_lock_pose(anim: Anim) -> bool {
    old_lock(anim)
        || second_variant(anim).is_some()
        || new_lock(anim).is_some_and(|(_, end)| end == End::Lock)
}

/// A side's class in the new locks: a single saber, the pair, the staff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Single,
    Pair,
    Staff,
}

impl Class {
    fn of(style: Style) -> Class {
        match style {
            Style::Fast | Style::Medium | Style::Strong => Class::Single,
            Style::Dual => Class::Pair,
            Style::Staff => Class::Staff,
        }
    }

    /// Its place among the second variants' classes.
    fn place(self) -> u16 {
        match self {
            Class::Single => 0,
            Class::Pair => 1,
            Class::Staff => 2,
        }
    }
}

/// A new lock's block: mine, theirs and the way (top or side).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LockBlock {
    mine: Class,
    theirs: Class,
    top: bool,
}

/// The five animations of a block, in their order in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    BreakLost,
    BreakWon,
    Lock,
    SuperLost,
    SuperWon,
}

/// The class pairs in the order their blocks come in the list.
const PAIRS: [(Class, Class); 9] = [
    (Class::Single, Class::Pair),
    (Class::Single, Class::Staff),
    (Class::Single, Class::Single),
    (Class::Pair, Class::Pair),
    (Class::Pair, Class::Staff),
    (Class::Pair, Class::Single),
    (Class::Staff, Class::Pair),
    (Class::Staff, Class::Staff),
    (Class::Staff, Class::Single),
];

/// The new locks' ends, in their order inside a block.
const ENDS: [End; 5] = [
    End::BreakLost,
    End::BreakWon,
    End::Lock,
    End::SuperLost,
    End::SuperWon,
];

impl LockBlock {
    /// The block's place in the list of blocks.
    fn index(self) -> usize {
        let pair = PAIRS
            .iter()
            .position(|&pair| pair == (self.mine, self.theirs))
            .unwrap_or(0);
        pair * 2 + usize::from(self.top)
    }

    /// The block's animation for `end`.
    fn anim(self, end: End) -> Anim {
        let at = self.index() * ENDS.len() + end as usize;
        BOTH_LK_S_DL_S_B_1_L.offset(at as i32)
    }
}

/// Which new lock block and end `anim` is, if it is one.
fn new_lock(anim: Anim) -> Option<(LockBlock, End)> {
    let first = BOTH_LK_S_DL_S_B_1_L;
    if !anim.between(first, BOTH_LK_ST_S_T_SB_1_W) {
        return None;
    }
    let at = usize::from(anim.index() - first.index());
    let (block, end) = (at / ENDS.len(), at % ENDS.len());
    let (mine, theirs) = PAIRS[block / 2];
    let block = LockBlock {
        mine,
        theirs,
        top: block % 2 == 1,
    };
    Some((block, ENDS[end]))
}

/// The second variant for two of `class`, top or side.
fn second_of(class: Class, top: bool) -> Anim {
    BOTH_LK_S_S_S_L_2.offset(i32::from(class.place() * 2 + u16::from(top)))
}

/// Which class and way a second variant is for, if `anim` is one.
fn second_variant(anim: Anim) -> Option<(Class, bool)> {
    let first = BOTH_LK_S_S_S_L_2;
    if !anim.between(first, BOTH_LK_ST_ST_T_L_2) {
        return None;
    }
    let at = anim.index() - first.index();
    let class = [Class::Single, Class::Pair, Class::Staff][usize::from(at / 2)];
    Some((class, at % 2 == 1))
}

/// How a lock starts: each side's animation and the fraction of it its
/// frame starts at, and the distance the two close to (rulebook 9.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opening {
    pub attacker: (Anim, f32),
    pub defender: (Anim, f32),
    pub distance: f32,
}

/// How a lock of `kind` opens between an attacker in style `att` and a
/// defender in `def`.
pub fn opening(kind: Kind, att: Style, def: Style) -> Opening {
    let (mine, theirs) = (Class::of(att), Class::of(def));
    if mine == Class::Single && theirs == Class::Single {
        return old_opening(kind);
    }
    let top = kind == Kind::Top;
    let mut attacker = LockBlock { mine, theirs, top }.anim(End::Lock);
    let mut defender = LockBlock {
        mine: theirs,
        theirs: mine,
        top,
    }
    .anim(End::Lock);
    if mine == theirs {
        if kind.attacker_second() {
            attacker = second_of(mine, top);
        } else {
            defender = second_of(mine, top);
        }
    }
    let row = kind.row();
    let attacker_at = if rises_to_win(attacker) {
        start::NEW_FAR[row]
    } else {
        start::NEW_NEAR[row]
    };
    let defender_at = if rises_to_win(defender) {
        start::NEW_NEAR[row]
    } else {
        start::NEW_FAR[row]
    };
    Opening {
        attacker: (attacker, attacker_at),
        defender: (defender, defender_at),
        distance: start::NEW_DISTANCE,
    }
}

/// The old locks of two single sabers: high, or circling one way or the
/// other.
fn old_opening(kind: Kind) -> Opening {
    let (attacker, defender) = match kind {
        Kind::Top => (BOTH_BF2LOCK, BOTH_BF1LOCK),
        Kind::TopRight | Kind::BottomLeft | Kind::R => (BOTH_CCWCIRCLELOCK, BOTH_CWCIRCLELOCK),
        Kind::TopLeft | Kind::BottomRight | Kind::L => (BOTH_CWCIRCLELOCK, BOTH_CCWCIRCLELOCK),
    };
    let at = start::OLD_START[kind.row()];
    let distance = if kind == Kind::Top {
        start::OLD_DISTANCE[0]
    } else {
        start::OLD_DISTANCE[1]
    };
    Opening {
        attacker: (attacker, at),
        defender: (defender, at),
        distance,
    }
}

/// The frame `fraction` of the way into `anim`.
pub fn first_frame(anims: &AnimTable, anim: Anim, fraction: f32) -> i32 {
    let info = anims.get(anim);
    info.first_frame + (info.frames as f32 * fraction) as i32
}

/// The hits a press banks: the style's level, more with a strong arm,
/// and a little luck.
pub fn press_hits(style: Style, strong_arm: bool, rng: &mut SimRng) -> u8 {
    let mut hits = power::style_level(style);
    if strong_arm {
        hits += contest::STRONG_ARM_HITS;
    }
    hits += roll_between(rng, 0, contest::RANDOM_HITS);
    hits.clamp(0, i32::from(u8::MAX)) as u8
}

/// A hit of `strength` spent: the lock's new frame and how many frames
/// are left to its winning end, or none when it got there (a win).
pub fn advance(anims: &AnimTable, lock: &Lock, strength: i32) -> Option<(i32, i32)> {
    let info = anims.get(lock.anim);
    let first = info.first_frame;
    let end = first + info.frames;
    if rises_to_win(lock.anim) {
        let frame = lock.frame + strength;
        (frame < end).then_some((frame, end - frame))
    } else {
        let frame = lock.frame - strength;
        (frame > first).then_some((frame, frame - first))
    }
}

/// The other side's frame on `anim` with `remaining` frames left to its
/// opponent's winning end.
pub fn mirror(anims: &AnimTable, anim: Anim, remaining: i32) -> i32 {
    let info = anims.get(anim);
    if rises_to_win(anim) {
        info.first_frame + remaining
    } else {
        (info.first_frame + info.frames) - remaining
    }
}

/// How one side comes out of a lock: the animation it plays (on the whole
/// body, held) and the move it takes, if any.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outcome {
    pub anim: Anim,
    pub saber_move: Option<Move>,
    /// A super-break's loser: its blade blocks nothing while this plays,
    /// held `end::DEFENCELESS_MS` more.
    pub defenceless: bool,
}

/// An outcome that plays `anim` and sets no move.
fn plays(anim: Anim) -> Outcome {
    Outcome {
        anim,
        saber_move: None,
        defenceless: false,
    }
}

/// An outcome that plays `anim` and takes `saber_move`.
fn takes(anim: Anim, saber_move: Move) -> Outcome {
    Outcome {
        anim,
        saber_move: Some(saber_move),
        defenceless: false,
    }
}

/// How the winner of an old lock comes out of it.
fn old_winner(winner: Anim, victory: bool, super_break: bool) -> Outcome {
    let high = matches!(winner, BOTH_BF2LOCK | BOTH_BF1LOCK);
    if super_break {
        return plays(if high {
            BOTH_LK_S_S_T_SB_1_W
        } else {
            BOTH_LK_S_S_S_SB_1_W
        });
    }
    match (winner, victory) {
        (BOTH_BF2LOCK, true) => takes(BOTH_A3_T__B_, Move::A_T2B),
        (BOTH_BF1LOCK, true) => takes(BOTH_K1_S1_T_, Move::K1_T_),
        (BOTH_BF2LOCK | BOTH_BF1LOCK, false) => plays(BOTH_BF1BREAK),
        (BOTH_CWCIRCLELOCK, true) => plays(BOTH_CWCIRCLEBREAK),
        (BOTH_CWCIRCLELOCK, false) => takes(BOTH_V1_BL_S1, Move::V1_BL),
        (BOTH_CCWCIRCLELOCK, true) => plays(BOTH_CCWCIRCLEBREAK),
        (_, _) => takes(BOTH_V1_BR_S1, Move::V1_BR),
    }
}

/// How the loser of a lock the winner played as an old one comes out.
fn old_loser(loser: Anim, victory: bool, super_break: bool) -> Outcome {
    let high = matches!(loser, BOTH_BF2LOCK | BOTH_BF1LOCK);
    if super_break && old_lock(loser) {
        return plays(if high {
            BOTH_LK_S_S_T_SB_1_L
        } else {
            BOTH_LK_S_S_S_SB_1_L
        });
    }
    match (loser, victory) {
        (BOTH_BF1LOCK, true) => plays(BOTH_KNOCKDOWN4),
        (BOTH_BF2LOCK | BOTH_BF1LOCK, _) => plays(BOTH_BF1BREAK),
        (BOTH_CWCIRCLELOCK, _) => takes(BOTH_V1_BL_S1, Move::V1_BL),
        (BOTH_CCWCIRCLELOCK, _) => takes(BOTH_V1_BR_S1, Move::V1_BR),
        (other, _) => plays(other),
    }
}

/// A side's animation stepped `by` places from its lock's first form.
fn stepped(anim: Anim, by: i32) -> Anim {
    let lock = match second_variant(anim) {
        Some((class, top)) => LockBlock {
            mine: class,
            theirs: class,
            top,
        }
        .anim(End::Lock),
        None => anim,
    };
    lock.offset(by)
}

/// How the two sides come out of a lock: the winner's (or, when nobody
/// won, the side that ended it) and the loser's. The old locks break by
/// their own tables; the new ones step from the lock to its block's
/// breaks.
pub fn breaks(winner: Anim, loser: Anim, victory: bool, super_break: bool) -> (Outcome, Outcome) {
    if old_lock(winner) {
        return (
            old_winner(winner, victory, super_break),
            old_loser(loser, victory, super_break),
        );
    }
    let from_lock = |end: End| end as i32 - End::Lock as i32;
    if super_break {
        let won = plays(stepped(winner, from_lock(End::SuperWon)));
        let lost = Outcome {
            anim: stepped(loser, from_lock(End::SuperLost)),
            saber_move: Some(Move::NONE),
            defenceless: true,
        };
        (won, lost)
    } else {
        (
            plays(stepped(winner, from_lock(End::BreakWon))),
            plays(stepped(loser, from_lock(End::BreakLost))),
        )
    }
}

/// Whether a win is a super-break: the winner's strength and banked hits
/// beat a roll.
pub fn super_breaks(strength: i32, hits: u8, rng: &mut SimRng) -> bool {
    let [low, high] = end::SUPER_BREAK_ROLL;
    strength + i32::from(hits) > roll_between(rng, low, high)
}

/// The push a lock's end gives along the way from `from` to `to`: a
/// loser's harder and lower than two sides parting.
pub fn shove(from: Vec3, to: Vec3, lost: bool) -> Vec3 {
    let mut way = math::sub(to, from);
    math::normalize(&mut way);
    let (push, lift) = if lost {
        (LOSS_PUSH, LOSS_LIFT)
    } else {
        (APART_PUSH, APART_LIFT)
    };
    [way[0] * push, way[1] * push, lift]
}

/// Whether the loser of a lock dies of it: likelier the weaker it is.
pub fn loss_kills(hp_percent: i32, rng: &mut SimRng) -> bool {
    roll_between(rng, 0, end::LOSS_KILL) > hp_percent
}
