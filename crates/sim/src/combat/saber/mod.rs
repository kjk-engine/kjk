//! Lightsabers and melee weapons: JA's move table (`moves`), and
//! what a character's saber is doing (`SaberState`), which the weapon step
//! inside move_body (`motion::Arms`) drives from its commands
//! (`docs/COMBAT.md`, C8, C9).
//! KOTOR's feats decide the styles a character has (`Kit`).
//!
//! What the server does with the blades each tick is in
//! the modules below and the step's `sabers` system (`world::sabers`):
//! - `blades`: where each blade is (the sim's skeleton), and its trail;
//! - `sweep`: a blade's sweep cut into traces, blade against blade;
//! - `power`: attack and animation power;
//! - `defence`: state blocks (C6), parries, bolts blocked;
//! - `clash`: the clash decision tree;
//! - `lock`: saber locks;
//! - `throw`: the thrown saber, and a saber knocked away;
//! - `kick`: the staff's kicks and the roll get-ups.

pub mod blades;
pub mod clash;
pub mod defence;
pub mod grace;
pub mod kick;
pub mod lock;
pub mod look;
pub mod moves;
pub mod power;
pub mod sweep;
pub mod throw;

pub use lock::Lock;
pub use moves::{Block, Hold, Move, MoveData, Quad};
pub use throw::Thrown;

use crate::combat::sheet::{Blaster, Delivery, SaberShape, Sheet, Weapon, feats};
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};
use rules::saber_moves::styles;

/// A saber style (JA's `saberAnimLevel`), numbered as JA numbers them.
/// Desann's and Tavion's (4 and 5) are left out: JA's multiplayer never
/// cycles to them, and no KOTOR feat maps to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Style {
    Fast = 1,
    Medium = 2,
    Strong = 3,
    Dual = 6,
    Staff = 7,
}

impl Style {
    pub fn from_index(index: u8) -> Option<Style> {
        Some(match index {
            1 => Style::Fast,
            2 => Style::Medium,
            3 => Style::Strong,
            6 => Style::Dual,
            7 => Style::Staff,
            _ => return None,
        })
    }

    /// Where the style's animations sit among the style groups (fast,
    /// medium, strong, dual, staff).
    pub fn group(self) -> u16 {
        match self {
            Style::Fast => 0,
            Style::Medium => 1,
            Style::Strong => 2,
            Style::Dual => 3,
            Style::Staff => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Style::Fast => "fast",
            Style::Medium => "medium",
            Style::Strong => "strong",
            Style::Dual => "dual",
            Style::Staff => "staff",
        }
    }
}

/// What the server told the saber happened to it this tick: nothing; a
/// move already chosen for it to play; its parry broken; its swing
/// bouncing off something; or one of the five parries, against a blade
/// (melee) or against a shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Blocked {
    #[default]
    None = 0,
    BounceMove,
    ParryBroken,
    AttackBounce,
    UpperRight,
    UpperLeft,
    LowerRight,
    LowerLeft,
    Top,
    UpperRightShot,
    UpperLeftShot,
    LowerRightShot,
    LowerLeftShot,
    TopShot,
}

impl Blocked {
    /// Every value in number order (snapshots store the number).
    const ALL: [Blocked; 14] = [
        Blocked::None,
        Blocked::BounceMove,
        Blocked::ParryBroken,
        Blocked::AttackBounce,
        Blocked::UpperRight,
        Blocked::UpperLeft,
        Blocked::LowerRight,
        Blocked::LowerLeft,
        Blocked::Top,
        Blocked::UpperRightShot,
        Blocked::UpperLeftShot,
        Blocked::LowerRightShot,
        Blocked::LowerLeftShot,
        Blocked::TopShot,
    ];

    /// One of the five parries against a blade (not a shot).
    pub fn melee_parry(self) -> bool {
        matches!(
            self,
            Blocked::UpperRight
                | Blocked::UpperLeft
                | Blocked::LowerRight
                | Blocked::LowerLeft
                | Blocked::Top
        )
    }
}

impl PartialOrd for Blocked {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Ordered by number: the later values outrank the earlier.
impl Ord for Blocked {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

/// Where a weapon change is (`weaponstate`): putting one away, raising the
/// next, or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Change {
    #[default]
    None = 0,
    Dropping,
    Raising,
}

/// JA's hand-extend state as far as a knockdown needs it
/// (`forceHandExtend`): down until `hand_until`, then getting up; once up
/// the weapon is raised again. The Force's gestures (I26) join it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hand {
    #[default]
    None = 0,
    Knockdown,
    /// Up again: the weapon rises (250 ms) on the next command.
    Risen,
}

/// How a knocked-down character gets up (`forceDodgeAnim`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Getup {
    /// Still lying.
    #[default]
    Down = 0,
    /// `BOTH_GETUP1`, 1 s.
    Plain,
    /// The force get-up (jump), 0.5 s.
    Force,
    /// A push's quick get-up, 0.5 s, 300 up.
    Quick,
}

/// What a character's saber (or melee weapon) is doing. Carried whole in
/// snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SaberState {
    /// The blades are lit (a melee weapon is always ready).
    pub on: bool,
    /// Of two blades, the second is out (JA's `saberHolstered` 1): the
    /// style button toggles it; the dual sabers then fight in the fast
    /// style.
    pub second_off: bool,
    /// The style it fights in, or last fought in; `None` until it first
    /// takes up a saber or melee weapon.
    pub style: Option<Style>,
    /// A style asked for while the weapon timer ran, applied once it is out.
    pub queued: Option<Style>,
    /// The move playing.
    pub current_move: Move,
    /// What the move blocks.
    pub blocking: Block,
    /// A block or clash to react to.
    pub blocked: Blocked,
    /// Attacks since the last ready (`saberAttackChainCount`, at most
    /// `rules::saber_moves::timing::CHAIN_CAP`).
    pub chain: u8,
    /// A weapon change under way.
    pub change: Change,
    /// Knocked down and getting up.
    pub hand: Hand,
    /// When the hand state ends (sim ms).
    pub hand_until: u64,
    pub getup: Getup,
    /// The next knockdown's get-up is the quick one (`quickerGetup`, a
    /// push's).
    pub quicker: bool,
    /// In a saber lock until then (sim ms; `saberLockTime`), and the lock
    /// (rulebook 9).
    pub lock_until: u64,
    pub lock: Option<Lock>,
    /// The style button was held on the last command (it cycles on a
    /// press).
    pub cycle_held: bool,
    /// Bolts are not blocked by the body again before then (sim ms; JA's
    /// per-level cooldown, rulebook 7.5).
    pub bolt_block_until: u64,
    /// The first saber away from the hand: thrown, or knocked out of it
    /// (rulebook 10.5 to 10.7).
    pub thrown: Option<Thrown>,
    /// Who the kick playing last struck, and until when (the kick's legs
    /// timer): one hit per victim per kick (rulebook 10.2).
    pub kicked: Option<(SimId, u64)>,
    /// Out until then (sim ms) after its own last attack, at someone or
    /// not, as a fight holds it out (`move_body`'s sheathing): a swing at
    /// no one is no fight (`Fighter::combat_until`).
    pub out_until: u64,
}

impl SaberState {
    /// Knocked down for `ms` from `now` (`G_Knockdown`: a kick's toss,
    /// `kick::DOWN_MS`, or a lost lock, `lock::LOSS_DOWN_MS`), unless down
    /// already.
    pub fn knock_down(&mut self, now: u64, ms: u64) {
        if self.hand == Hand::Knockdown {
            return;
        }
        self.hand = Hand::Knockdown;
        self.hand_until = now + ms;
        self.getup = Getup::Down;
    }

    /// Up from a knockdown at once, its weapon ready.
    pub fn stand_up(&mut self) {
        if self.hand == Hand::Knockdown {
            self.hand = Hand::None;
            self.getup = Getup::Down;
        }
    }

    /// Down or getting up (`HANDEXTEND_KNOCKDOWN`).
    pub fn floored(&self) -> bool {
        self.hand == Hand::Knockdown
    }

    /// A special plays: the Force pool does not refill (rulebook 2.12).
    pub fn in_special(&self) -> bool {
        self.current_move.special()
    }

    /// The saber flies (thrown or on its way back): the Force pool does not
    /// refill either (rulebook 10.5).
    pub fn flying(&self) -> bool {
        self.thrown.is_some_and(|thrown| thrown.flying())
    }

    /// In a saber lock at `now`.
    pub fn locked(&self, now: u64) -> bool {
        self.lock.is_some() && self.lock_until > now
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(u8::from(self.on));
        out.u8(u8::from(self.second_off));
        out.u8(self.style.map_or(0, |style| style as u8));
        out.u8(self.queued.map_or(0, |style| style as u8));
        out.u8(self.current_move as u8);
        out.u8(self.blocking as u8);
        out.u8(self.blocked as u8);
        out.u8(self.chain);
        out.u8(self.change as u8);
        out.u8(self.hand as u8);
        out.u64(self.hand_until);
        out.u8(self.getup as u8);
        out.u8(u8::from(self.quicker));
        out.u64(self.lock_until);
        write_fixed(out, self.lock.as_ref(), &Lock::BLANK, Lock::encode);
        out.u8(u8::from(self.cycle_held));
        out.u64(self.bolt_block_until);
        write_fixed(out, self.thrown.as_ref(), &Thrown::BLANK, Thrown::encode);
        write_fixed(
            out,
            self.kicked.as_ref(),
            &(SimId(0), 0),
            |(victim, until), out| {
                out.u32(victim.0);
                out.u64(*until);
            },
        );
        out.u64(self.out_until);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<SaberState, DecodeError> {
        fn pick<T: Copy>(
            input: &mut Reader,
            what: &'static str,
            all: &[T],
        ) -> Result<T, DecodeError> {
            let at = input.at();
            let index = input.u8()?;
            all.get(usize::from(index))
                .copied()
                .ok_or(input.invalid(what, at))
        }
        fn style(input: &mut Reader, what: &'static str) -> Result<Option<Style>, DecodeError> {
            let at = input.at();
            match input.u8()? {
                0 => Ok(None),
                index => Style::from_index(index)
                    .map(Some)
                    .ok_or(input.invalid(what, at)),
            }
        }
        let flag = |input: &mut Reader, what| pick(input, what, &[false, true]);
        Ok(SaberState {
            on: flag(input, "saber on flag")?,
            second_off: flag(input, "second blade flag")?,
            style: style(input, "saber style")?,
            queued: style(input, "queued saber style")?,
            current_move: pick(input, "saber move", Move::ALL)?,
            blocking: pick(
                input,
                "saber blocking",
                &[Block::No, Block::Tight, Block::Wide],
            )?,
            blocked: pick(input, "saber blocked", &Blocked::ALL)?,
            chain: input.u8()?,
            change: pick(
                input,
                "weapon change",
                &[Change::None, Change::Dropping, Change::Raising],
            )?,
            hand: pick(
                input,
                "hand state",
                &[Hand::None, Hand::Knockdown, Hand::Risen],
            )?,
            hand_until: input.u64()?,
            getup: pick(
                input,
                "get-up",
                &[Getup::Down, Getup::Plain, Getup::Force, Getup::Quick],
            )?,
            quicker: flag(input, "quick get-up flag")?,
            lock_until: input.u64()?,
            lock: read_fixed(input, &Lock::BLANK, Lock::decode, "lock flag")?,
            cycle_held: flag(input, "style button flag")?,
            bolt_block_until: input.u64()?,
            thrown: read_fixed(input, &Thrown::BLANK, Thrown::decode, "thrown flag")?,
            kicked: read_fixed(
                input,
                &(SimId(0), 0),
                |input| Ok((SimId(input.u32()?), input.u64()?)),
                "kicked flag",
            )?,
            out_until: input.u64()?,
        })
    }
}

/// A weapon that swings with the saber move tables: a lightsaber, or a
/// sword, vibroblade or staff (C9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blade {
    /// A lightsaber (it lights; bolts are blocked by it in a later slice).
    pub saber: bool,
    /// Two-bladed: the staff style.
    pub double: bool,
    /// Its wielder has the base item's proficiency feat (or it needs none).
    pub proficient: bool,
}

impl Blade {
    /// The blade `weapon` is, if it is one, in `sheet`'s hands: lightsabers
    /// and the melee base items; two-bladed are the double-bladed lightsaber
    /// and KOTOR's `weaponwield` 3 melee rows (quarterstaff, double-bladed
    /// sword, vibro double-blade).
    pub fn of(weapon: &Weapon, sheet: &Sheet) -> Option<Blade> {
        let (saber, double) = match weapon.delivery {
            Delivery::Saber(shape) => (true, shape == SaberShape::Staff),
            Delivery::Melee => (false, matches!(weapon.base_item, 0 | 6 | 7)),
            _ => return None,
        };
        let proficient = weapon
            .proficiency
            .is_none_or(|feat| sheet.feats.has(feat) || sheet.feats.has(feats::PROFICIENCY_ALL));
        Some(Blade {
            saber,
            double,
            proficient,
        })
    }
}

/// What a hand holds, as the weapon step sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Held {
    /// An empty left hand (an empty right hand has its fists).
    #[default]
    Nothing,
    Blade(Blade),
    /// A blaster: its cadence runs its weapon timer (`weapons::cadence`).
    Blaster(Blaster),
    /// Fists or a stun baton.
    Other,
}

/// What a character's sheet gives its sabers (I22 C8): what its hands
/// hold, the tiers of its melee feat lines, its Force pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Kit {
    pub hands: [Held; 2],
    /// Critical Strike (medium, `styles::MEDIUM_LEVELS`), Flurry (fast), Power Attack
    /// (strong), Two-Weapon Fighting (dual or staff) tiers.
    pub critical: u8,
    pub flurry: u8,
    pub power: u8,
    pub two_weapon: u8,
    /// Jedi Defense tier: how long a parry holds.
    pub jedi_defense: u8,
    /// Force Jump tier: the staff's back-flip attack needs
    /// `rules::saber_moves::specials::BACK_FLIP_JUMP`.
    pub force_jump: u8,
    /// Dueling tier: more offence in clashes and locks with one saber and
    /// an empty off hand (C8, `styles::DUELING`).
    pub dueling: u8,
    /// The saber throw it knows (`throw::level`), if any.
    pub throw: Option<Throw>,
    pub max_fp: u16,
    /// The second bar specials are paid from (`Sheet::pool`).
    pub pool: u16,
}

/// A saber throw a character knows: its level, and what it costs
/// (hundredths; the `spells.2da` row's `forcepoints`, which the world
/// reads from the module, `rules::saber_throw::out::COST` without one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Throw {
    pub level: u8,
    pub row: u16,
    pub cost: i32,
}

impl Kit {
    pub fn of(sheet: &Sheet) -> Kit {
        let held = |hand: usize| match (hand, sheet.weapons[hand]) {
            (_, Some(weapon)) => match weapon.delivery {
                Delivery::Blaster(blaster) => Held::Blaster(blaster),
                _ => Blade::of(&weapon, sheet).map_or(Held::Other, Held::Blade),
            },
            (0, None) => Held::Other,
            (_, None) => Held::Nothing,
        };
        Kit {
            hands: [held(0), held(1)],
            critical: sheet.feats.count(&feats::CRITICAL_STRIKE),
            flurry: sheet.feats.count(&feats::FLURRY),
            power: sheet.feats.count(&feats::POWER_ATTACK),
            two_weapon: sheet.feats.count(&feats::TWO_WEAPON_FIGHTING),
            jedi_defense: sheet.feats.count(&feats::JEDI_DEFENSE),
            force_jump: sheet.feats.count(&feats::FORCE_JUMP),
            dueling: sheet.feats.count(&feats::DUELING),
            throw: throw::level(&sheet.powers).map(|(level, row)| Throw {
                level,
                row,
                cost: rules::saber_throw::out::COST,
            }),
            max_fp: sheet.max_fp,
            pool: sheet.pool(),
        }
    }

    pub fn held(&self, hand: u8) -> Held {
        self.hands
            .get(usize::from(hand))
            .copied()
            .unwrap_or(Held::Nothing)
    }

    /// The blade in `hand`, if it holds one.
    pub fn blade(&self, hand: u8) -> Option<Blade> {
        match self.held(hand) {
            Held::Blade(blade) => Some(blade),
            _ => None,
        }
    }

    /// The style `hand`'s blade forces: the staff for a two-bladed one, the
    /// dual style when the other hand holds a blade too; `None` for one
    /// single blade, which fights in any style it has.
    pub fn forced_style(&self, hand: u8) -> Option<Style> {
        let blade = self.blade(hand)?;
        if blade.double {
            Some(Style::Staff)
        } else if self.blade(1 - hand.min(1)).is_some() {
            Some(Style::Dual)
        } else {
            None
        }
    }

    /// The offence level of `style` with `blade` (C8; JA's saber offence
    /// for the style).
    pub fn level(&self, style: Style, blade: Blade) -> u8 {
        match style {
            Style::Fast => self.flurry,
            Style::Medium => match self.critical.min(3) {
                0 => u8::from(blade.proficient),
                tier => styles::MEDIUM_LEVELS[usize::from(tier) - 1],
            },
            Style::Strong => self.power,
            Style::Dual | Style::Staff => self.two_weapon,
        }
    }

    /// The single-blade styles open to `blade`, in cycling order (fast,
    /// medium, strong); the medium style when none is (KOTOR's plain
    /// attack, at level 0).
    pub fn singles(&self, blade: Blade) -> Vec<Style> {
        let open: Vec<Style> = [Style::Fast, Style::Medium, Style::Strong]
            .into_iter()
            .filter(|style| self.level(*style, blade) > 0)
            .collect();
        if open.is_empty() {
            vec![Style::Medium]
        } else {
            open
        }
    }

    /// The style a character takes up `hand`'s blade in: the forced one;
    /// else the one it last fought in, if the blade has it (JA keeps the
    /// style across weapon changes); else medium if it has it (KOTOR's
    /// plain attack), else its first.
    pub fn style_for(&self, hand: u8, last: Option<Style>) -> Option<Style> {
        self.forced_style(hand)
            .or_else(|| self.single_for(hand, last))
    }

    /// The single-blade style `hand`'s blade fights in alone (a staff with
    /// its second blade out too): the one it last fought in, if it has
    /// it; else medium if it has it, else its first.
    pub fn single_for(&self, hand: u8, last: Option<Style>) -> Option<Style> {
        let blade = self.blade(hand)?;
        let singles = self.singles(blade);
        Some(last.filter(|style| singles.contains(style)).unwrap_or(
            if singles.contains(&Style::Medium) {
                Style::Medium
            } else {
                singles[0]
            },
        ))
    }

    /// A single blade's style after `style` when cycling
    /// (`Cmd_SaberAttackCycle_f`): the next one up it has, wrapping to its
    /// first.
    pub fn next_single(&self, hand: u8, style: Style) -> Option<Style> {
        let blade = self.blade(hand)?;
        let singles = self.singles(blade);
        Some(
            singles
                .iter()
                .copied()
                .find(|next| *next > style)
                .unwrap_or(singles[0]),
        )
    }

    /// Two blades fighting as one in `style` (JA's secondary style): no
    /// specials but the fast lunge.
    pub fn secondary(&self, hand: u8, style: Style) -> bool {
        self.forced_style(hand)
            .is_some_and(|forced| forced != style)
    }

    /// The offence a clash or a lock counts for `style` with `blade`: the
    /// style's level, `styles::DUELING` more with Dueling and one blade
    /// alone (C8).
    pub fn offence(&self, style: Style, blade: Blade, hand: u8) -> u8 {
        let alone = self.forced_style(hand).is_none() && !blade.double;
        let dueling = if self.dueling > 0 && alone {
            styles::DUELING
        } else {
            0
        };
        self.level(style, blade) + dueling
    }

    /// `weapon` as a blade swung in `style` carries it (I22 C8): the style's
    /// KOTOR melee feat attack, shaped as `specials::Special::shot` shapes a
    /// blaster's shot. Power Attack in the strong style: more damage at a
    /// cost to hit (`styles::POWER_ATTACK`); Critical Strike in the medium:
    /// a wider threat range (`styles::CRITICAL_THREAT`); Flurry in the fast:
    /// less to hit (`styles::FLURRY_PENALTY`; its extra attack is the
    /// style's pace). The dual and staff styles take none; without the feat
    /// a style swings `weapon` as it is.
    pub fn swing(&self, style: Style, mut weapon: Weapon) -> Weapon {
        let tier = |held: u8| usize::from(held.min(3)).checked_sub(1);
        match style {
            Style::Strong => {
                if let Some(tier) = tier(self.power) {
                    let bonus = styles::POWER_ATTACK[tier];
                    weapon.damage_bonus = weapon.damage_bonus.saturating_add(bonus);
                    weapon.attack_bonus = weapon
                        .attack_bonus
                        .saturating_sub(styles::POWER_ATTACK_PENALTY);
                }
            }
            Style::Medium => {
                if let Some(tier) = tier(self.critical) {
                    weapon.crit_threat = weapon
                        .crit_threat
                        .max(1)
                        .saturating_mul(tier as u8 + styles::CRITICAL_THREAT);
                }
            }
            Style::Fast => {
                if let Some(tier) = tier(self.flurry) {
                    let penalty = styles::FLURRY_PENALTY[tier.min(1)];
                    weapon.attack_bonus = weapon.attack_bonus.saturating_sub(penalty);
                }
            }
            Style::Dual | Style::Staff => {}
        }
        weapon
    }

    /// The parry's defence level (JA's saber defence): Jedi Defense with a
    /// lightsaber, level 1 for a proficient melee wielder (C9).
    pub fn defence(&self, blade: Blade) -> u8 {
        if blade.saber {
            self.jedi_defense
        } else {
            u8::from(blade.proficient)
        }
    }

    /// What a special costing `percent` of the bar costs, in hundredths
    /// (I22 C7: JA's cost as a fraction of the pool, FP or stamina);
    /// `None` without a bar, which cannot pay for any.
    pub fn cost(&self, percent: u16) -> Option<i32> {
        (self.pool > 0).then(|| i32::from(self.pool) * i32::from(percent))
    }
}
