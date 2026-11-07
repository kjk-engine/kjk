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
//! the same ones.

use crate::wire::{DecodeError, Reader, Writer};

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
    /// The block button was held on the last command.
    pub block_held: bool,
    /// A press of attack with block held opened the perfect window until
    /// then.
    pub perfect_until: u64,
    /// When the last perfect block was made: what the view slows time for.
    pub perfect_at: u64,
    /// MDA's riposte: the next attack quickened until then.
    pub riposte_until: u64,
    /// The meter does not refill before then.
    pub refill_at: u64,
    /// The mishap count does not fall before then.
    pub decay_at: u64,
    /// No disarm takes the saber before then (the easy difficulties').
    pub guard_until: u64,
    /// The swing playing cannot be blocked: what the warning tints.
    pub warn: bool,
    /// How worn the blade's hilt is, and whether it broke.
    pub wear: u16,
    pub broken: bool,
}

impl Duel {
    /// Full, unhurried meters: a saber new to a fight, and every saber in
    /// a save made before the duel modes.
    pub const FRESH: Duel = Duel {
        points: rules::duel::meters::POINTS_MAX,
        mishap: 0,
        fatigued: false,
        block_held: false,
        perfect_until: 0,
        perfect_at: 0,
        riposte_until: 0,
        refill_at: 0,
        decay_at: 0,
        guard_until: 0,
        warn: false,
        wear: 0,
        broken: false,
    };

    pub(crate) fn encode(&self, out: &mut Writer) {
        let Duel {
            points,
            mishap,
            fatigued,
            block_held,
            perfect_until,
            perfect_at,
            riposte_until,
            refill_at,
            decay_at,
            guard_until,
            warn,
            wear,
            broken,
        } = *self;
        out.u8(points);
        out.u8(mishap);
        out.u8(u8::from(fatigued)
            | u8::from(block_held) << 1
            | u8::from(warn) << 2
            | u8::from(broken) << 3);
        for time in [
            perfect_until,
            perfect_at,
            riposte_until,
            refill_at,
            decay_at,
            guard_until,
        ] {
            out.u64(time);
        }
        out.u16(wear);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Duel, DecodeError> {
        let points = input.u8()?;
        let mishap = input.u8()?;
        let at = input.at();
        let flags = input.u8()?;
        if flags >> 4 != 0 {
            return Err(input.invalid("duel flags", at));
        }
        let mut times = [0u64; 6];
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
        ] = times;
        Ok(Duel {
            points,
            mishap,
            fatigued: flags & 1 != 0,
            block_held: flags & 2 != 0,
            perfect_until,
            perfect_at,
            riposte_until,
            refill_at,
            decay_at,
            guard_until,
            warn: flags & 4 != 0,
            wear: input.u16()?,
            broken: flags & 8 != 0,
        })
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
