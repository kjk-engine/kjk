//! The duel rules a session plays its blades by (`docs/SABER-RULES.md`):
//! JA's own (the default), or one of the two duel modes players know from
//! the Movie Duels mod, re-made here from a description of how they play.
//!
//! - **JA** (`SaberRules::Ja`): the blades block by state, as JA's do.
//! - **Duel** (`SaberRules::Duel`, "MD"): a block button, perfect blocks,
//!   mishaps and fatigue; the blocks are paid from the Force pool.
//! - **Duel advanced** (`SaberRules::DuelAdvanced`, "MDA"): the same, paid
//!   from a meter of block points of its own, so the Force pool is left to
//!   the powers; the block button holds its own poses.
//!
//! The rules are the session's (the host's), kept in the world and in its
//! snapshots (`Snapshot::saber_rules`), so every seat and every save plays
//! the same ones. Every number they play by is the rulebook's chapter
//! `duel` (`data/rules/duel.toml`).
//!
//! - `meter`: the meters, what moves cost, what gives back, the refill,
//!   mishaps, fatigue and wear;
//! - `exchange`: what a blow meeting a blade decides (the block states,
//!   the costs, the bounce ladder, the mishap roll);
//! - `shots`: bolts against the blade.
//!
//! The world applies them (`world::sabers`, the weapon step, the Force's
//! upkeep, the minds), each path read only under a duel mode.

pub mod exchange;
pub mod meter;
pub mod shots;

use super::{Kit, Move, Style};
use crate::math::{self, Vec3};
use crate::wire::{DecodeError, Reader, Writer};
use rules::duel::block;
use rules::duel::difficulty;
use rules::duel::disarm;
use rules::duel::master;
use rules::duel::perfect;
use rules::duel::speed;

/// Which rules the blades play by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum SaberRules {
    /// JA's.
    #[default]
    Ja = 0,
    /// The duel mode, paid from the Force pool ("MD").
    Duel = 1,
    /// The advanced duel mode, paid from block points ("MDA").
    DuelAdvanced = 2,
}

impl SaberRules {
    /// Every value in number order (snapshots store the number).
    pub const ALL: [SaberRules; 3] = [SaberRules::Ja, SaberRules::Duel, SaberRules::DuelAdvanced];

    /// The short name a flag, a setting or a report uses.
    pub fn name(self) -> &'static str {
        match self {
            SaberRules::Ja => "ja",
            SaberRules::Duel => "md",
            SaberRules::DuelAdvanced => "mda",
        }
    }

    /// The name a menu shows.
    pub fn title(self) -> &'static str {
        match self {
            SaberRules::Ja => "Jedi Academy",
            SaberRules::Duel => "Movie Duels",
            SaberRules::DuelAdvanced => "Movie Duels Advanced",
        }
    }

    /// The rules named `name` (`name`'s), any case.
    pub fn named(name: &str) -> Option<SaberRules> {
        SaberRules::ALL
            .into_iter()
            .find(|rules| rules.name().eq_ignore_ascii_case(name))
    }

    /// One of the duel modes: the block button, perfect blocks, mishaps.
    pub fn duel(self) -> bool {
        self != SaberRules::Ja
    }

    /// Blocks are paid from block points of their own (MDA), not from the
    /// Force pool (MD).
    pub fn own_meter(self) -> bool {
        self == SaberRules::DuelAdvanced
    }

    /// The next rules in a menu's cycle.
    pub fn next(self) -> SaberRules {
        SaberRules::ALL[(self as usize + 1) % SaberRules::ALL.len()]
    }

    /// An entry of a rule kept by mode, MD's first: this mode's.
    pub fn of<T: Copy>(self, by_mode: [T; 2]) -> T {
        by_mode[usize::from(self.own_meter())]
    }

    pub(crate) fn encode(self, out: &mut Writer) {
        out.u8(self as u8);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<SaberRules, DecodeError> {
        let at = input.at();
        let index = input.u8()?;
        SaberRules::ALL
            .get(usize::from(index))
            .copied()
            .ok_or(input.invalid("saber rules", at))
    }
}

/// Why a reaction move plays, for the duel attack speed's paces
/// (`rules::duel::speed::BOUNCES`): none, or one of its entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pace {
    #[default]
    Own = 0,
    Blocked,
    Parried,
    Staggered,
    Perfect,
}

impl Pace {
    const ALL: [Pace; 5] = [
        Pace::Own,
        Pace::Blocked,
        Pace::Parried,
        Pace::Staggered,
        Pace::Perfect,
    ];
}

/// A fighter's last hit point (hundredths): at it a saber user blocks no
/// more (`docs/SABER-RULES.md` 2.2, 10).
pub const LAST_HEALTH: i32 = 100;

/// How many flags a duel's encoding packs.
const FLAGS: usize = 11;

/// A saber's duel meters and timers (`SaberState::duel`): kept under JA's
/// rules too, where nothing moves them. Sim ms throughout; 0 for a time
/// never set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duel {
    /// Block points left (MDA's own meter; MD pays from the Force pool).
    pub points: u8,
    /// The mishap count: swings blocked and exchanges lost pile it up, and
    /// it ends in bounces, staggers and disarms.
    pub mishap: u8,
    /// Fatigued: the meter fell too low (no acrobatic get-up, lunges and
    /// spins not blocked).
    pub fatigued: bool,
    /// Drained: a cost left the meter low (lunges and spins not blocked).
    pub drained: bool,
    /// The block button held on the last command, and blocking by it.
    pub block_held: bool,
    /// Attack held with it: the aimed block (and its perfect window).
    pub aimed: bool,
    /// The swing under way is an attack fake.
    pub fake: bool,
    /// The kick key was held on the last command (it kicks on a press).
    pub kick_held: bool,
    /// A creature's block stance (MDA): it holds its ground facing its foe.
    pub stance: bool,
    /// Sprinting (block held while running).
    pub sprint: bool,
    /// A scene held it on its last step (a scene's start refills it).
    pub scened: bool,
    /// The swing playing cannot be blocked: what the warning tints.
    pub warn: bool,
    /// The hilt wore through and broke: a staff fights with one blade.
    pub broken: bool,
    /// A press of attack with block held opened the perfect window until
    /// then.
    pub perfect_until: u64,
    /// When the last perfect block was made: what the view slows time for.
    pub perfect_at: u64,
    /// MDA's riposte: the next attack quickened until then.
    pub riposte_until: u64,
    /// The block points do not refill before then.
    pub refill_at: u64,
    /// The mishap count does not fall before then.
    pub decay_at: u64,
    /// No disarm takes the saber before then (the easy difficulties').
    pub guard_until: u64,
    /// When the aimed block began.
    pub aimed_at: u64,
    /// The Force does not refill before then (block held, a busy creature).
    pub force_at: u64,
    /// The next of the duel's own refill ticks (fatigue, crouching).
    pub tick_at: u64,
    /// A creature does not meditate again before then.
    pub meditate_at: u64,
    /// When the hilt last wore.
    pub wear_at: u64,
    /// A creature absorbs kicks until then.
    pub kick_guard_until: u64,
    /// The sprint fuel's next point burnt or regained.
    pub fuel_at: u64,
    /// Sprint fuel left.
    pub fuel: u8,
    /// Why the next reaction move plays (the duel attack speed's pace).
    pub pace: Pace,
    /// How worn the blade's hilt is.
    pub wear: u16,
}

impl Duel {
    /// Full, unhurried meters: a saber new to a fight, and every saber in
    /// a save made before the duel modes.
    pub const FRESH: Duel = Duel {
        points: rules::duel::meters::POINTS_MAX,
        mishap: 0,
        fatigued: false,
        drained: false,
        block_held: false,
        aimed: false,
        fake: false,
        kick_held: false,
        stance: false,
        sprint: false,
        scened: false,
        warn: false,
        broken: false,
        perfect_until: 0,
        perfect_at: 0,
        riposte_until: 0,
        refill_at: 0,
        decay_at: 0,
        guard_until: 0,
        aimed_at: 0,
        force_at: 0,
        tick_at: 0,
        meditate_at: 0,
        wear_at: 0,
        kick_guard_until: 0,
        fuel_at: 0,
        fuel: rules::duel::sprint::FUEL[0],
        pace: Pace::Own,
        wear: 0,
    };

    /// The flags in bit order.
    fn flags(&self) -> [bool; FLAGS] {
        [
            self.fatigued,
            self.block_held,
            self.warn,
            self.broken,
            self.drained,
            self.aimed,
            self.fake,
            self.kick_held,
            self.stance,
            self.sprint,
            self.scened,
        ]
    }

    /// The times in order.
    fn times(&self) -> [u64; 13] {
        [
            self.perfect_until,
            self.perfect_at,
            self.riposte_until,
            self.refill_at,
            self.decay_at,
            self.guard_until,
            self.aimed_at,
            self.force_at,
            self.tick_at,
            self.meditate_at,
            self.wear_at,
            self.kick_guard_until,
            self.fuel_at,
        ]
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.points);
        out.u8(self.mishap);
        let bits = (self.flags().iter().enumerate())
            .fold(0u16, |bits, (i, &on)| bits | u16::from(on) << i);
        out.u16(bits);
        for time in self.times() {
            out.u64(time);
        }
        out.u8(self.fuel);
        out.u8(self.pace as u8);
        out.u16(self.wear);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Duel, DecodeError> {
        let points = input.u8()?;
        let mishap = input.u8()?;
        let at = input.at();
        let bits = input.u16()?;
        if bits >> FLAGS != 0 {
            return Err(input.invalid("duel flags", at));
        }
        let flag = |i: u16| bits >> i & 1 != 0;
        let mut times = [0u64; 13];
        for time in &mut times {
            *time = input.u64()?;
        }
        let [
            perfect_until,
            perfect_at,
            riposte_until,
            refill_at,
            decay_at,
            guard_until,
            aimed_at,
            force_at,
            tick_at,
            meditate_at,
            wear_at,
            kick_guard_until,
            fuel_at,
        ] = times;
        let fuel = input.u8()?;
        let at = input.at();
        let pace = *Pace::ALL
            .get(usize::from(input.u8()?))
            .ok_or(input.invalid("duel pace", at))?;
        Ok(Duel {
            points,
            mishap,
            fatigued: flag(0),
            block_held: flag(1),
            warn: flag(2),
            broken: flag(3),
            drained: flag(4),
            aimed: flag(5),
            fake: flag(6),
            kick_held: flag(7),
            stance: flag(8),
            sprint: flag(9),
            scened: flag(10),
            perfect_until,
            perfect_at,
            riposte_until,
            refill_at,
            decay_at,
            guard_until,
            aimed_at,
            force_at,
            tick_at,
            meditate_at,
            wear_at,
            kick_guard_until,
            fuel_at,
            fuel,
            pace,
            wear: input.u16()?,
        })
    }

    /// The block it holds by the button (`exchange::Guard`): in the perfect
    /// window, aimed, held, or none.
    pub fn guard(&self, now: u64) -> exchange::Guard {
        use exchange::Guard;
        match (self.block_held, self.aimed) {
            (false, _) => Guard::None,
            (true, true) if now < self.perfect_until => Guard::Perfect,
            (true, true) => Guard::Active,
            (true, false) => Guard::Held,
        }
    }

    /// Lunges and spins get past it (fatigued or drained).
    pub fn weary(&self) -> bool {
        self.fatigued || self.drained
    }
}

impl Default for Duel {
    fn default() -> Duel {
        Duel::FRESH
    }
}

impl std::fmt::Display for SaberRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for SaberRules {
    type Err = String;

    fn from_str(name: &str) -> Result<SaberRules, String> {
        SaberRules::named(name).ok_or_else(|| format!("{name}: not ja, md or mda"))
    }
}

/// The attacks only a perfect block stops (`docs/SABER-RULES.md` 6.1, the
/// spec's animations as this move table names them): the back attacks,
/// the butterflies, the jump and flip attacks, the spins, the long leap,
/// the stabs down, the katas, the pull's attacks and the pair's.
pub fn unblockable(mv: Move) -> bool {
    use Move::*;
    mv.kata()
        || matches!(
            mv,
            A_BACK
                | A_BACKSTAB
                | A_BACK_CR
                | A_BACKFLIP_ATK
                | BUTTERFLY_LEFT
                | BUTTERFLY_RIGHT
                | JUMPATTACK_STAFF_LEFT
                | JUMPATTACK_STAFF_RIGHT
                | JUMPATTACK_DUAL
                | A_JUMP_T__B_
                | A_FLIP_STAB
                | A_FLIP_SLASH
                | SPINATTACK
                | SPINATTACK_DUAL
                | SPINATTACK_ALORA
                | LEAP_ATTACK
                | STABDOWN
                | STABDOWN_STAFF
                | STABDOWN_DUAL
                | PULL_ATTACK_STAB
                | PULL_ATTACK_SWING
                | DUAL_FB
                | DUAL_LR
        )
}

/// A lunge or a spin: what a fatigued or drained blocker cannot stop.
pub fn lunge_or_spin(mv: Move) -> bool {
    matches!(
        mv,
        Move::A_LUNGE | Move::SPINATTACK | Move::SPINATTACK_DUAL | Move::SPINATTACK_ALORA
    )
}

/// A heavy style's blow (JA's strong; the spec's Desann's too, which KOTOR
/// has not).
pub fn heavy(style: Style) -> bool {
    style == Style::Strong
}

/// A master of the blade (the spec's master classes, KOTOR's own): Master
/// Jedi Defense (`master::DEFENSE`), or `master::LEVEL` levels.
pub fn master(kit: &Kit, level: u8) -> bool {
    kit.jedi_defense >= master::DEFENSE || level >= master::LEVEL
}

/// How long the perfect window stays open at the duel's difficulty.
pub fn window() -> u64 {
    perfect::WINDOW_MS[difficulty::LEVEL.min(perfect::WINDOW_MS.len() - 1)]
}

/// Whether the disarm guard refuses a disarm at `now` (`docs/SABER-RULES.md`
/// 4.6): the first starts the guard and is refused, as is any during it;
/// one after it (within as long again) takes the saber, and a later one
/// starts a guard anew. None at the hard difficulty.
pub fn guarded(duel: &mut Duel, now: u64) -> bool {
    let guard = disarm::GUARD_MS[difficulty::LEVEL.min(disarm::GUARD_MS.len() - 1)];
    if guard == 0 {
        return false;
    }
    if duel.guard_until == 0 || now >= duel.guard_until + guard {
        duel.guard_until = now + guard;
        return true;
    }
    if now < duel.guard_until {
        return true;
    }
    duel.guard_until = 0;
    false
}

/// Whether `from` lies in front of one at `origin` facing `yaw`
/// (`block::CONE`, the flat way's dot with the facing).
pub fn in_front(origin: Vec3, yaw: f32, from: Vec3) -> bool {
    let (forward, _, _) = math::angle_vectors([0.0, yaw, 0.0]);
    let mut way = math::sub(from, origin);
    way[2] = 0.0;
    math::normalize(&mut way);
    math::dot(way, forward) > block::CONE
}

/// What holding the block button needs of the one holding it
/// (`docs/SABER-RULES.md` 2.2), as the weapon step sees it.
#[derive(Debug, Clone, Copy)]
pub struct Hold {
    /// A lightsaber in hand (lit, or to be lit), not thrown.
    pub blade: bool,
    /// Its Jedi Defense tier.
    pub defence: u8,
    /// Its meter and Force (points, `meter::meter`).
    pub meter: i32,
    pub force: i32,
    pub on_ground: bool,
    /// Down, rolling, locked, in a special, a bounce, a knockaway or a
    /// broken move, swinging, or pressing another attack or a power.
    pub busy: bool,
}

/// Whether the block button blocks (`docs/SABER-RULES.md` 2.2).
pub fn can_hold(hold: &Hold) -> bool {
    use rules::duel::meters;
    hold.blade
        && hold.defence >= 1
        && hold.meter >= meters::LEAST
        && hold.force >= meters::LEAST
        && hold.on_ground
        && !hold.busy
}

/// Whether a block held while running sprints (`meters::RUN_LEAST`).
pub fn runs_blocking(meter: i32, fuel: u8) -> bool {
    meter >= rules::duel::meters::RUN_LEAST && fuel > rules::duel::sprint::FUEL[1]
}

/// The pace a saber move plays at beyond its own under a duel mode:
/// quicker as a riposte (MDA); with the duel attack speed
/// (`speed::OPTION`), by style, slowed by why a reaction plays (`pace`)
/// and by attack fatigue.
pub fn play_speed(duel: &Duel, style: Option<Style>, next: Move, now: u64) -> f32 {
    let mut factor = 1.0;
    if next.attack() && duel.riposte_until > now {
        factor /= perfect::RIPOSTE_TIME;
    }
    if !speed::OPTION {
        return factor;
    }
    let style_pace = match style {
        Some(Style::Fast) => speed::STYLES[0],
        Some(Style::Medium) | None => speed::STYLES[1],
        Some(Style::Strong) => speed::STYLES[2],
        Some(Style::Dual) => speed::STYLES[3],
        Some(Style::Staff) => speed::STYLES[4],
    };
    let reaction = next.bounce() || next.broken() || next.ret();
    let paced = match duel.pace {
        Pace::Own => None,
        pace => Some(speed::BOUNCES[pace as usize - 1]),
    };
    factor
        * match paced.filter(|_| reaction) {
            Some(pace) => pace,
            None if next.attack() && duel.mishap > rules::duel::mishap::FAST_ABOVE => {
                speed::BOUNCES[4]
            }
            None => style_pace,
        }
}
