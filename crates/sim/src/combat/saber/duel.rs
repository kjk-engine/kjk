//! The duel rules a session plays its blades by (`docs/SABER-RULES.md`):
//! JA's own (the default), or the duel mode players know from the Movie
//! Duels mod, re-made here from a description of how it plays.
//!
//! - **JA** (`SaberRules::Ja`): the blades block by state, as JA's do.
//! - **Duel advanced** (`SaberRules::DuelAdvanced`, "MDA"): a block
//!   button, perfect blocks, mishaps and fatigue, paid from a meter of block
//!   points of its own, so the Force pool is left to the powers.
//!
//! Movie Duels' plain mode (MD, blocks paid from the Force pool) is not
//! offered: a snapshot, replay or setting that names it plays MDA.
//!
//! The rules are the session's (the host's), kept in the world and in its
//! snapshots (`Snapshot::saber_rules`), so every seat and every replay plays
//! the same ones; a save loaded into play takes the host's setting
//! (`game.saber_rules`) again, as the rules are the player's choice, not
//! the story's. Every number they play by is the rulebook's chapter
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
#[cfg(feature = "probes")]
pub mod tap;

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
    /// kjk's own Advanced rules (formerly "MDA"). Its number stays 2: 1 was
    /// the plain duel mode's (MD), read as this one.
    DuelAdvanced = 2,
}

impl SaberRules {
    /// Every value a menu offers, in its order.
    pub const ALL: [SaberRules; 2] = [SaberRules::Ja, SaberRules::DuelAdvanced];

    /// The short name a flag, a setting or a report uses.
    pub fn name(self) -> &'static str {
        match self {
            SaberRules::Ja => "ja",
            SaberRules::DuelAdvanced => "advanced",
        }
    }

    /// The name a menu shows.
    pub fn title(self) -> &'static str {
        match self {
            SaberRules::Ja => "Jedi Academy",
            SaberRules::DuelAdvanced => "Advanced",
        }
    }

    /// The names the Advanced rules had before they were kjk's own: Movie
    /// Duels' advanced mode ("mda") and its plain mode ("md").
    pub const RETIRED_NAMES: &'static [&'static str] = &["mda", "md"];

    /// The rules named `name`, in any case. The Advanced rules answer to
    /// their retired names too.
    pub fn named(name: &str) -> Option<SaberRules> {
        let is = |wanted: &str| name.eq_ignore_ascii_case(wanted);
        SaberRules::ALL
            .into_iter()
            .find(|rules| is(rules.name()))
            .or_else(|| {
                SaberRules::RETIRED_NAMES
                    .iter()
                    .any(|old| is(old))
                    .then_some(SaberRules::DuelAdvanced)
            })
    }

    /// The duel mode: the block button, perfect blocks, mishaps.
    pub fn duel(self) -> bool {
        self != SaberRules::Ja
    }

    /// The next rules in a menu's cycle.
    pub fn next(self) -> SaberRules {
        let at = SaberRules::ALL
            .iter()
            .position(|&rules| rules == self)
            .unwrap_or(0);
        SaberRules::ALL[(at + 1) % SaberRules::ALL.len()]
    }

    pub(crate) fn encode(self, out: &mut Writer) {
        out.u8(self as u8);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<SaberRules, DecodeError> {
        let at = input.at();
        match input.u8()? {
            0 => Ok(SaberRules::Ja),
            // 1 was Movie Duels' plain mode (MD), no longer offered.
            1 | 2 => Ok(SaberRules::DuelAdvanced),
            _ => Err(input.invalid("saber rules", at)),
        }
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

/// How many flags a duel's encoding packs. Bit 9 is unused and always 0.
const FLAGS: usize = 11;

/// How many times a duel's encoding holds.
const TIMES: usize = 14;

/// The perfect parries a fighter may play (`skeleton::stance::PERFECTS`).
const PERFECTS: usize = crate::skeleton::stance::PERFECTS;

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
    /// Why the next reaction move plays (the duel attack speed's pace).
    pub pace: Pace,
    /// How worn the blade's hilt is.
    pub wear: u16,
    /// The perfect parry the torso plays (`docs/SABER-RULES.md` 3.1): 0
    /// for none, else 1 and its index in `skeleton::stance::PERFECT`.
    pub parry: u8,
    /// When the perfect parry began.
    pub parry_at: u64,
    /// When the perfect parry ends: it plays until then.
    pub parry_until: u64,
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
        pace: Pace::Own,
        wear: 0,
        parry: 0,
        parry_at: 0,
        parry_until: 0,
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
            false,
            self.scened,
        ]
    }

    /// The times in order.
    fn times(&self) -> [u64; TIMES] {
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
            self.parry_at,
            self.parry_until,
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
        out.u8(self.pace as u8);
        out.u16(self.wear);
        out.u8(self.parry);
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
        // Before the perfect parry (`v0.17.1-md-rc.1`'s saves): the last
        // time was the sprint's fuel timer, a fuel byte followed the times,
        // and no parry was kept. Both sprint fields are dropped, as the
        // sprint flag (bit 9) is.
        let parries = input.since(crate::since::snapshot::STANCE_PARRIES);
        let mut times = [0u64; TIMES];
        let kept = if parries { TIMES } else { TIMES - 2 };
        for time in &mut times[..kept] {
            *time = input.u64()?;
        }
        if !parries {
            input.u64()?;
            input.u8()?;
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
            parry_at,
            parry_until,
        ] = times;
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
            pace,
            wear: input.u16()?,
            parry: {
                let at = input.at();
                match parries.then(|| input.u8()).transpose()? {
                    None => 0,
                    Some(parry) if usize::from(parry) <= PERFECTS => parry,
                    Some(_) => return Err(input.invalid("duel parry", at)),
                }
            },
            parry_at,
            parry_until,
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
        SaberRules::named(name).ok_or_else(|| format!("{name}: not ja or advanced"))
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
    mastery(kit, level) >= master::DEFENSE
}

/// How far toward a master `kit`'s holder of `level` levels is: its Jedi
/// Defense tier (0 to `master::DEFENSE`), the top from `master::LEVEL`.
pub fn mastery(kit: &Kit, level: u8) -> u8 {
    if level >= master::LEVEL {
        master::DEFENSE
    } else {
        kit.jedi_defense.min(master::DEFENSE)
    }
}

/// The share (percent) a holder at `mastery` pays or waits of the usual,
/// where a master's is `share`: all of it at the first Jedi Defense tier,
/// the master's at the top, evenly between.
pub fn toward_master(mastery: u8, share: i64) -> i64 {
    let steps = i64::from(mastery.saturating_sub(1));
    let top = i64::from(master::DEFENSE.saturating_sub(1)).max(1);
    100 - (100 - share) * steps / top
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

/// Where a blow met at `point` (units) meets a blocker standing at
/// `origin` with its eyes `eye_height` above it and facing `yaw`
/// (`docs/SABER-RULES.md` 3.1). Behind is tested first, and only for a
/// blocker that `guards_behind` (one with Jedi Defense). Then the point's
/// height against the eyes gives the band, and its side against the
/// blocker's right gives the side.
pub fn parry_met(
    origin: Vec3,
    eye_height: f32,
    yaw: f32,
    point: Vec3,
    guards_behind: bool,
) -> crate::skeleton::Met {
    use crate::skeleton::Met;
    let (forward, right, _) = math::angle_vectors([0.0, yaw, 0.0]);
    let flat = |from: Vec3| {
        let mut way = math::sub(point, from);
        way[2] = 0.0;
        math::normalize(&mut way);
        way
    };
    if guards_behind && math::dot(flat(origin), forward) <= perfect::BEHIND {
        return Met::Behind;
    }
    let eyes = [origin[0], origin[1], origin[2] + eye_height];
    let height = point[2] - eyes[2];
    let side = math::dot(flat(eyes), right);
    let [upper, middle] = perfect::BANDS;
    let edge = if height > upper {
        perfect::SIDES[0]
    } else if height >= middle {
        perfect::SIDES[1]
    } else {
        return if side >= 0.0 {
            Met::LowRight
        } else {
            Met::LowLeft
        };
    };
    if side > edge {
        Met::Right
    } else if side < -edge {
        Met::Left
    } else {
        Met::Centre
    }
}

/// The weapon a perfect parry's table column names (`skeleton::stance::
/// PERFECT`): two blades for the dual style, a staff for the staff style,
/// and one blade for every other.
pub fn parry_blades(style: Option<Style>) -> usize {
    match style {
        Some(Style::Dual) => 1,
        Some(Style::Staff) => 2,
        _ => 0,
    }
}

/// A perfect parry's weapon timer (`docs/SABER-RULES.md` 3.1): a roll of
/// `roll` between `perfect::PARRY_WAIT_MS`'s shortest and longest.
pub fn parry_wait(roll: impl FnOnce(i32, i32) -> i32) -> i32 {
    let [shortest, longest] = perfect::PARRY_WAIT_MS;
    roll(shortest, longest)
}

/// What holding the block button needs of the one holding it
/// (`docs/SABER-RULES.md` 2.2), as the weapon step sees it.
#[derive(Debug, Clone, Copy)]
pub struct Hold {
    /// A blade in hand (a lightsaber lit or to be lit, or a melee blade),
    /// not thrown.
    pub blade: bool,
    /// Its defence level (`Kit::defence`): a lightsaber's Jedi Defense
    /// tier, a melee blade's 1 when proficient with it.
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

/// Whether a block held while running blocks (`meters::RUN_LEAST`).
pub fn runs_blocking(meter: i32) -> bool {
    meter >= rules::duel::meters::RUN_LEAST
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
