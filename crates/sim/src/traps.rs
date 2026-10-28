//! Traps and mines (`docs/COMBAT.md`): KOTOR's
//! rules for finding, disarming, recovering and laying them, and what a
//! trap is (`Trap`, a `traps.2da` row's `TrapRow`). Pure: the authority
//! applies them (`world::traps`).

use crate::module::ObjectData;
use rules::interaction::checks;
use rules::traps::detect;
use rules::traps::disarm;
use rules::traps::door_mines;
use rules::traps::lay;
use rules::traps::recover;

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;

/// How often every creature looks for traps near it (ms), and how far
/// (metres, from the trap's `RADIUS_M` circle).
pub const DETECT_EVERY_MS: u64 = rules::traps::detect::EVERY_MS;
pub const DETECT_RANGE_M: f32 = rules::traps::detect::RANGE_M;
/// `ranges.2da` `TrapRadius` (`ModuleData::ranges`), K1's where a level
/// has none.
pub const RADIUS_M: f32 = crate::module::Ranges::K1.trap_radius_m;
/// How long disarming or recovering takes, and laying (ms).
pub const DISARM_MS: u64 = rules::traps::disarm::MS;
pub const LAY_MS: u64 = rules::traps::lay::MS;
/// The most traps the party may have laid in an area.
pub const LAID_MOST: usize = rules::traps::lay::MOST;
/// A laid mine's area: the square this far each way from where it stands
/// (metres).
pub const LAID_HALF_M: f32 = rules::traps::lay::HALF_M;
/// A disarm DC past this never yields.
pub const HOPELESS_DC: i16 = rules::traps::disarm::HOPELESS_DC;
/// What a laid mine's template is named (no resref holds `*`).
pub const LAID_PREFIX: &str = "*mine";

/// A `traps.2da` row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrapRow {
    pub label: String,
    /// `trapscript`: what springing it runs where the trap names none.
    pub script: String,
    /// `setdc`, `detectdcmod`, `disarmdcmod`.
    pub set_dc: u8,
    pub detect_mod: u8,
    pub disarm_mod: u8,
    /// `trapname` (a strref).
    pub name: u32,
    /// `resref`: the kit recovering it gives (empty for none).
    pub kit: String,
    /// `model` and `explosionsound`, lowercase.
    pub model: String,
    pub sound: String,
}

/// A trap's facts: its `traps.2da` row and its template's trap fields.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Trap {
    pub row: u8,
    pub detectable: bool,
    pub disarmable: bool,
    /// KOTOR's "flagged" trap: seen by every creature without a roll (no
    /// K1 template sets it; scripts and later games may).
    pub flagged: bool,
    pub one_shot: bool,
    pub detect_dc: u8,
    pub disarm_dc: u8,
    /// `KeyName`, lowercase; empty for none.
    pub key: String,
    pub auto_remove_key: bool,
}

/// How a disarm or recover went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Failed,
    /// Failed badly: the trap springs on whoever tried.
    Sprung,
}

/// A laid mine's DCs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Laid {
    pub detect_dc: u8,
    pub disarm_dc: u8,
}

/// Whether a creature finds a trap: Awareness total + its die
/// (`detect::DIE`) + `detect::BASE`, `detect::RUNNING` less while running,
/// reaching its detect DC.
pub fn detects(awareness: i16, d10: u8, running: bool, dc: u8) -> bool {
    awareness + i16::from(d10) + detect::BASE - if running { detect::RUNNING } else { 0 }
        >= i16::from(dc)
}

/// The sum of a check: the die rolled (in combat), else `checks::TAKEN`.
fn sum(total: i16, roll: Option<u8>) -> i16 {
    i16::from(roll.unwrap_or(checks::TAKEN)) + total
}

/// Disarming: a trap the party made yields; else the sum against the DC
/// (0 counts as 1), never past `HOPELESS_DC`; short by more than
/// `disarm::SPRINGS_SHORT_BY`, it springs.
pub fn disarm(total: i16, roll: Option<u8>, dc: u8, own: bool) -> Outcome {
    if own {
        return Outcome::Done;
    }
    let (got, dc) = (sum(total, roll), i16::from(dc.max(1)));
    if dc <= HOPELESS_DC && got >= dc {
        Outcome::Done
    } else if got < dc - disarm::SPRINGS_SHORT_BY {
        Outcome::Sprung
    } else {
        Outcome::Failed
    }
}

/// Recovering: as disarming against `recover::HARDER` more; short by more
/// than `recover::SPRINGS_SHORT_BY`, it springs.
pub fn recover(total: i16, roll: Option<u8>, dc: u8, own: bool) -> Outcome {
    if own {
        return Outcome::Done;
    }
    let (got, dc) = (
        sum(total, roll),
        i16::from(dc.max(1)) + i16::from(recover::HARDER),
    );
    if got >= dc {
        Outcome::Done
    } else if got < dc - recover::SPRINGS_SHORT_BY {
        Outcome::Sprung
    } else {
        Outcome::Failed
    }
}

/// Laying a mine of `row`: the sum (+`lay::RANKS_BONUS` with
/// `lay::RANKS_FOR_BONUS` base ranks) against its `setdc`; the mine's DCs
/// its row's mods plus the sum. A failure lays nothing, but in combat (a
/// roll) a failure by more than `lay::LAYS_ANYWAY_SHORT_BY` lays it all the
/// same (KOTOR's, Q-0142).
pub fn lay(total: i16, base_ranks: u8, roll: Option<u8>, row: &TrapRow) -> (i16, Option<Laid>) {
    let got = sum(total, roll)
        + if base_ranks >= lay::RANKS_FOR_BONUS {
            lay::RANKS_BONUS
        } else {
            0
        };
    let set = i16::from(row.set_dc);
    let laid = got >= set || (roll.is_some() && got < set - lay::LAYS_ANYWAY_SHORT_BY);
    let dc = |modifier: u8| (i16::from(modifier) + got).clamp(0, 255) as u8;
    (
        got,
        laid.then(|| Laid {
            detect_dc: dc(row.detect_mod),
            disarm_dc: dc(row.disarm_mod),
        }),
    )
}

/// Whether a creature walking in springs a trap: hostile (`standing`
/// toward its creator, else its faction, `HOSTILE_AT_MOST` or less), not
/// of its faction, not immune to traps.
pub fn springs(standing: u8, same_faction: bool, immune: bool) -> bool {
    standing <= rules::world::factions::HOSTILE_AT_MOST && !same_faction && !immune
}

/// Of kits' `traps.2da` rows, the one laid first: the deadliest tier (a
/// row's tier is its place in its type's three), then the later row. The
/// rock falls (12, 13) have no kit.
pub fn best_kit(rows: impl IntoIterator<Item = u8>) -> Option<u8> {
    rows.into_iter()
        .filter(|row| *row < 12)
        .max_by_key(|row| (row % 3, *row))
}

/// TSL's mine on a door (its exe's set-mine action and door events): set
/// never rolled (in combat too), it blasts the lock `BLAST_MS` later, and a
/// blasted door opens `OPEN_AFTER_MS` after that, unlocked, even a plot
/// door (no `NotBlastable` is read).
pub const BLAST_MS: u64 = rules::traps::door_mines::BLAST_MS;
pub const OPEN_AFTER_MS: u64 = rules::traps::door_mines::OPEN_AFTER_MS;

/// A kit's blast strength against a lock, by its `traps.2da` row: TSL's
/// exe's table for rows 4 to 26 (`door_mines::STRENGTH`), none for the
/// rest.
pub fn blast_strength(row: u8) -> u8 {
    row.checked_sub(4)
        .and_then(|at| door_mines::STRENGTH.get(usize::from(at)))
        .copied()
        .unwrap_or(0)
}

/// A blast on a lock: the sum (`door_mines::BLAST_BASE`, the setter's
/// Demolitions over `door_mines::DEMOLITIONS_DIVISOR`, rounded toward zero,
/// and the mine's strength) and whether it reaches the lock's open DC
/// (TSL's line 128535 shows it).
pub fn blasts(demolitions: i16, strength: u8, dc: u8) -> (i16, bool) {
    let sum = door_mines::BLAST_BASE
        + demolitions / door_mines::DEMOLITIONS_DIVISOR
        + i16::from(strength);
    (sum, sum >= i16::from(dc))
}

/// The kit a player sets on a door with lock DC `dc`: the weakest whose
/// blast opens it, else the strongest (TSL's menu lists each kit; the
/// choice is the player's there).
pub fn door_kit(demolitions: i16, dc: u8, rows: impl IntoIterator<Item = u8>) -> Option<u8> {
    let rows: Vec<u8> = rows.into_iter().collect();
    let key = |row: &u8| (blast_strength(*row), *row);
    let opens = rows
        .iter()
        .filter(|row| blasts(demolitions, blast_strength(**row), dc).1)
        .min_by_key(|row| key(row));
    opens
        .or_else(|| rows.iter().max_by_key(|row| key(row)))
        .copied()
}

/// The template a mine of `row` laid in play is made from.
pub fn laid_template(row: u8) -> String {
    format!("{LAID_PREFIX}{row}")
}

/// The facts of a mine of `template` (`laid_template`) laid in play or
/// spawned by a script: its row's trap script and trap (detectable,
/// disarmable, one shot), no outline (`LAID_HALF_M`).
pub fn laid_facts(module: &crate::module::ModuleData, template: &str) -> Option<ObjectData> {
    use crate::module::ObjectKind;
    let row = laid_row(template)?;
    let facts = module.traps.get(usize::from(row))?;
    Some(ObjectData {
        kind: ObjectKind::Trigger,
        scripts: vec![(
            crate::script::Hook::TrapTriggered
                .label(crate::script::Holder::Object(ObjectKind::Trigger))?
                .into(),
            facts.script.clone(),
        )],
        trap: Some(Trap {
            row,
            detectable: true,
            disarmable: true,
            one_shot: true,
            ..Trap::default()
        }),
        ..ObjectData::unknown(template, [0.0; 3], 0.0)
    })
}

/// The row of a laid mine's template.
pub fn laid_row(template: &str) -> Option<u8> {
    template.strip_prefix(LAID_PREFIX)?.parse().ok()
}

/// KOTOR's own animations for a skill's use on a mine (its disarm,
/// recover and set actions; no script constant names them), drawn as the
/// body's `disablemine` (`DISARM_MS`, as long as the use) and `setmine`
/// (`LAY_MS`).
pub mod animations {
    pub const DISARM: u16 = 10132;
    pub const RECOVER: u16 = 10141;
    pub const SET: u16 = 10140;
}

/// A trap's middle (metres): its outline's mean, relative to `position`;
/// `position` for one without an outline (a laid mine).
pub fn centre(geometry: &[[f32; 3]], position: [f32; 3]) -> [f32; 3] {
    if geometry.is_empty() {
        return position;
    }
    let n = geometry.len() as f32;
    let mut sum = [0.0f32; 3];
    for point in geometry {
        for (total, value) in sum.iter_mut().zip(point) {
            *total += value;
        }
    }
    std::array::from_fn(|axis| position[axis] + sum[axis] / n)
}
