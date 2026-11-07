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

