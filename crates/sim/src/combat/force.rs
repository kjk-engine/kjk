//! The Force (I26): the pool and its regeneration (I22 C13), and each
//! power's JA shape, timing and physics with KOTOR's powers deciding who
//! knows what at which level and what a cast costs (C10, C12;
//! `docs/COMBAT.md`). This module owns `ForceState`, its encoding and the
//! powers' numbers; the step's `force` system (`world::force`) runs them.
//!
//! **KOTOR's impact is its script where it runs.** In KOTOR every power's
//! effect on a target is its `spells.2da` impact script (`k_sp1_generic`);
//! where a level has it, the delivery here only finds whom a power catches
//! and the script does the rest (`world::powers`). Where no script can run
//! (a stage without the games' scripts, a test), a power's damage and
//! healing are JA's own numbers on JA's 100-point scale, turned into the
//! target's pools (a JA point is 1% of the target's maximum hit points, or
//! force points), so a power keeps JA's weight against any KOTOR body.
//! Costs are KOTOR's (`spells.2da` `forcepoints`,
//! `ModuleData::force_costs`); the crowd control rows (stun, stasis, fear
//! and their chains) are KOTOR effects with a saving throw, held by C11.
//!
//! How a character casts (`Command::force` and the force buttons):
//! - `buttons::FORCE_POWER` casts the selected power (`Command::force`: a
//!   `spells.2da` row the character knows, or `PULL` for JA's pull, known
//!   with push). Any row of a chain casts the character's best level of it
//!   (I22's chains become JA levels); a crowd-control row casts itself.
//!   Instant and toggled powers act on the press; a toggle pressed again
//!   (`ja::TOGGLE_MIN_MS` or more after it started) stops.
//! - Grip, lightning and drain run while their button is held (JA's own
//!   buttons, or the power button with them selected).

use crate::combat::fighter::{decode_id, encode_id};
use crate::combat::sheet::Powers;
use crate::script::Menu;
use crate::script::effects::Hold;
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

/// The JA powers KOTOR's chains map onto (I22 section 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Power {
    Push,
    Pull,
    Lightning,
    Drain,
    Grip,
    Speed,
    /// KOTOR's Cure and Heal: JA's team heal (I22's table).
    Heal,
    Protect,
    Absorb,
    MindTrick,
    /// KOTOR's own crowd control (stun, stasis, fear, …): a KOTOR effect
    /// delivered at the crosshair target or over an area (C11).
    Stun,
    /// TSL's Fury: JA's rage.
    Rage,
    /// TSL's Force Sight: JA's sight.
    Sight,
    /// KOTOR's own effect powers JA has no shape for (`combat::effects`):
    /// Valor on the caster's side, Energy Resistance on the caster (its
    /// side at level 2), Slow's chain and Supress / Breach at the one aimed
    /// at.
    Valor,
    Resist,
    Slow,
    Dispel,
    /// A power a menu of the game offers that JA has no shape for (TSL's
    /// Force Barrier, Battle Meditation, Breath Control, Revitalize, ...):
    /// its impact script does all it does, cast on the caster from its own
    /// menu, or at the one aimed at from a foe's (`script::Menu`).
    Scripted,
}

/// Whether a power whose `spells.2da` row has the masks `(forbid,
/// require)` (`forbiditemmask`, `requireitemmask`) is barred to a
/// character wearing the items `worn` (`Gear::force_items`), as both
/// games' executables decide it when a cast begins: barred when any item
/// worn is one the row forbids (armour, for most powers that work on the
/// caster or its side), or when an item the row requires is not worn (a
/// lightsaber, for the saber throw). Their radial menus grey such a power.
pub fn items_bar((forbid, require): (u8, u8), worn: u8) -> bool {
    worn & forbid != 0 || worn & require != require
}

/// `Command::force` for JA's pull, which KOTOR has no row for: known at
/// the level of push. Row 255 is no Force power in either game's
/// `spells.2da`.
pub const PULL: u16 = 255;

/// The `spells.2da` row whose icon, cost, description and target the power
/// row `row` uses. Pull has no row of its own, so it uses Force Push's row.
/// Every other power uses its own row.
pub fn data_row(row: u16) -> u16 {
    match row {
        PULL => crate::combat::spells::FORCE_POWER_FORCE_PUSH,
        row => row,
    }
}

/// The JA power and level a `spells.2da` row gives (I22's chain table,
/// and TSL's own powers where JA has their shape: Drain Force I–III is JA's
/// drain, which takes force points; Master Heal the heal's third level;
/// Force Crush the grip's third; Mind Trick and Confusion the mind trick's
/// two; Force Scream an area stun; Fury JA's rage; Force Sight JA's
/// sight). Force Deflection and Force Redirection are always in effect
/// and are never cast (`deflection`). Valor, Energy Resistance, Slow's
/// chain and Supress / Breach are KOTOR effects delivered at the caster,
/// its side or the one aimed at (`combat::effects`); TSL's powers JA has no
/// shape for (Battle Meditation, the barriers, Revitalize, ...) map to
/// none here, and their impact scripts alone cast them
/// (`Power::Scripted`, `selected`). Keyed by the rows' labels
/// (`combat::spells`), which K1 and TSL share.
pub fn of_row(row: u16) -> Option<(Power, u8)> {
    use crate::combat::spells::*;
    Some(match row {
        FORCE_POWER_DRAIN_FORCE => (Power::Drain, 1),
        FORCE_POWER_IMPROVED_DRAIN_FORCE => (Power::Drain, 2),
        FORCE_POWER_MASTER_DRAIN_FORCE => (Power::Drain, 3),
        FORCE_POWER_MASTER_HEAL => (Power::Heal, 3),
        FORCE_POWER_FORCE_CRUSH => (Power::Grip, 3),
        FORCE_POWER_MIND_TRICK => (Power::MindTrick, 1),
        FORCE_POWER_CONFUSION => (Power::MindTrick, 2),
        FORCE_POWER_FORCE_SCREAM
        | FORCE_POWER_IMPROVED_FORCE_SCREAM
        | FORCE_POWER_MASTER_FORCE_SCREAM => (Power::Stun, 1),
        FORCE_POWER_FURY => (Power::Rage, 1),
        FORCE_POWER_IMPROVED_FURY => (Power::Rage, 2),
        FORCE_POWER_MASTER_FURY => (Power::Rage, 3),
        FORCE_POWER_FORCE_SIGHT => (Power::Sight, 1),
        FORCE_POWER_FORCE_PUSH => (Power::Push, 1),
        FORCE_POWER_FORCE_WHIRLWIND => (Power::Push, 2),
        FORCE_POWER_FORCE_WAVE => (Power::Push, 3),
        FORCE_POWER_SHOCK => (Power::Lightning, 1),
        FORCE_POWER_LIGHTNING => (Power::Lightning, 2),
        FORCE_POWER_FORCE_STORM => (Power::Lightning, 3),
        FORCE_POWER_DRAIN_LIFE => (Power::Drain, 2),
        FORCE_POWER_DEATH_FIELD => (Power::Drain, 3),
        FORCE_POWER_WOUND => (Power::Grip, 1),
        FORCE_POWER_CHOKE => (Power::Grip, 2),
        FORCE_POWER_KILL => (Power::Grip, 3),
        FORCE_POWER_SPEED_BURST => (Power::Speed, 1),
        FORCE_POWER_KNIGHT_SPEED => (Power::Speed, 2),
        FORCE_POWER_SPEED_MASTERY => (Power::Speed, 3),
        FORCE_POWER_CURE => (Power::Heal, 1),
        FORCE_POWER_HEAL => (Power::Heal, 2),
        FORCE_POWER_FORCE_AURA => (Power::Protect, 1),
        FORCE_POWER_FORCE_SHIELD => (Power::Protect, 2),
        FORCE_POWER_FORCE_ARMOR => (Power::Protect, 3),
        FORCE_POWER_RESIST_FORCE => (Power::Absorb, 1),
        FORCE_POWER_FORCE_IMMUNITY => (Power::Absorb, 2),
        FORCE_POWER_AFFECT_MIND => (Power::MindTrick, 1),
        FORCE_POWER_DOMINATE => (Power::MindTrick, 2),
        FORCE_POWER_STUN
        | FORCE_POWER_STASIS
        | FORCE_POWER_MASS_STASIS
        | FORCE_POWER_FEAR
        | FORCE_POWER_HORROR
        | FORCE_POWER_INSANITY
        | FORCE_POWER_DROID_STUN
        | FORCE_POWER_DROID_DISABLE
        | FORCE_POWER_DROID_DESTROY => (Power::Stun, 1),
        FORCE_POWER_FORCE_VALOR => (Power::Valor, 1),
        FORCE_POWER_KNIGHT_VALOR => (Power::Valor, 2),
        FORCE_POWER_MASTER_VALOR => (Power::Valor, 3),
        FORCE_POWER_RESIST_ENERGY_1 => (Power::Resist, 1),
        FORCE_POWER_RESIST_ENERGY_2 => (Power::Resist, 2),
        FORCE_POWER_SLOW => (Power::Slow, 1),
        FORCE_POWER_AFFLICTION => (Power::Slow, 2),
        FORCE_POWER_PLAGUE => (Power::Slow, 3),
        FORCE_POWER_SUPRESS_FORCE => (Power::Dispel, 1),
        FORCE_POWER_FORCE_BREACH => (Power::Dispel, 2),
        _ => return None,
    })
}

/// Whether `power` is cast on the caster or the caster's side rather than
/// on an enemy: the heals, speed, protection, absorption, valor, energy
/// resistance, rage and sight. Push, pull, lightning, drain, grip, mind
/// trick, the crowd control powers, slow and dispel are used on enemies.
pub fn on_own_side(power: Power) -> bool {
    matches!(
        power,
        Power::Heal
            | Power::Speed
            | Power::Protect
            | Power::Absorb
            | Power::Valor
            | Power::Resist
            | Power::Rage
            | Power::Sight
    )
}

/// TSL's Force Deflection and Force Redirection (`spells.2da` rows
/// `FORCE_POWER_FORCE_REPULSION` and `FORCE_POWER_FORCE_REDIRECTION`).
/// Neither is cast: TSL's rows have no impact script and mark both powers
/// as always in effect. TSL's own check of whether a projectile can be
/// parried lets a character who knows either one turn a blaster bolt that
/// comes from in front, whatever the character holds. Only Force
/// Redirection lets the bolt go back at its shooter, and it adds to every
/// deflection roll the character makes. Here a character who knows one
/// turns bolts without a lit lightsaber (`world::sabers::bolt_block`).
pub mod deflection {
    use crate::combat::Sheet;
    use crate::combat::saber::defence;
    use crate::combat::spells::{FORCE_POWER_FORCE_REDIRECTION, FORCE_POWER_FORCE_REPULSION};

    /// What Force Redirection adds to the deflection bonus of every weapon
    /// `sheet` holds: nothing without it.
    pub fn bonus(sheet: &Sheet) -> i8 {
        if sheet.powers.has(FORCE_POWER_FORCE_REDIRECTION) {
            rules::force::deflection::REDIRECTION_BONUS
        } else {
            0
        }
    }

    /// The bolt level `sheet` turns bolts at without a lightsaber, with
    /// the deflection bonus `deflect` (its weapon's, `bonus` included); 0
    /// when it knows neither power. Force Deflection never reaches the
    /// level that sends a bolt back at its shooter.
    pub fn bolt_level(sheet: &Sheet, deflect: i8) -> u8 {
        let [deflection, redirection] = rules::force::deflection::LEVEL;
        if sheet.powers.has(FORCE_POWER_FORCE_REDIRECTION) {
            defence::bolt_level(redirection, deflect)
        } else if sheet.powers.has(FORCE_POWER_FORCE_REPULSION) {
            defence::bolt_level(deflection, deflect).min(deflection)
        } else {
            0
        }
    }
}

/// KOTOR's Force power DC for `caster` (the engine's `GetSpellSaveDC`;
/// StrategyWiki's K1 rule): the book's `force.rolls.dc_base` + level +
/// WIS + CHA + Force Focus (`feats::FORCE_FOCUS_DC` by tier).
pub fn save_dc(caster: &crate::combat::Sheet) -> i32 {
    use crate::combat::sheet::{Ability, feats};
    let focus = feats::FORCE_FOCUS
        .iter()
        .zip(feats::FORCE_FOCUS_DC)
        .filter(|(feat, _)| caster.feats.has(**feat))
        .map(|(_, bonus)| bonus)
        .max()
        .unwrap_or(0);
    rules::force::rolls::DC_BASE
        + i32::from(caster.level)
        + caster.modifier(Ability::Wisdom)
        + caster.modifier(Ability::Charisma)
        + focus
}

/// One's Force resistance (`ResistForce`; 0 for none): the book's
/// `force.rolls` base + a share a tier + its level under Resist Force or
/// Force Immunity (absorb `tier` 1 or 2), or what effects grant it
/// (`granted`), the higher. A caster breaks it when d20 + its level
/// reaches it.
pub fn resistance(level: u8, tier: u8, granted: i32) -> i32 {
    let from_absorb = match tier.min(2) {
        0 => 0,
        tier => {
            use rules::force::rolls;
            rolls::RESISTANCE_BASE + rolls::RESISTANCE_PER_TIER * i32::from(tier) + i32::from(level)
        }
    };
    from_absorb.max(granted)
}

/// The crowd-control rows: which reach an area (the rest the crosshair
/// target), which frighten (ended by damage, C11), which only droids
/// feel (the rest only the living), the save each allows and the feat
/// that makes one immune (`k_sp1_generic`'s saves; the Sentinel's Force
/// Immunity feats), by the rows' labels. Force Scream's three allow a Will
/// save, as TSL's `k_sp1_generic` asks of them.
pub mod crowd {
    use crate::combat::sheet::feats;
    use crate::combat::spells::*;

    /// The save against `row` (`Sheet::save`'s index): Will for the fear
    /// chain and Force Scream, else Fortitude.
    pub fn save(row: u16) -> usize {
        if fear(row) || scream(row) { 2 } else { 0 }
    }

    /// Force Scream's three rows: a target that fails its save covers its
    /// ears (`ForceState::cover_ears`).
    pub fn scream(row: u16) -> bool {
        matches!(
            row,
            FORCE_POWER_FORCE_SCREAM
                | FORCE_POWER_IMPROVED_FORCE_SCREAM
                | FORCE_POWER_MASTER_FORCE_SCREAM
        )
    }

    /// The `IMMUNITY_TYPE_*` that keeps `row` off (paralysis for stasis,
    /// fear for the fear chain, stun for the rest); mind-affecting
    /// immunity (`MIND_SPELLS`) keeps them all off.
    pub fn immunity_type(row: u16) -> u8 {
        match row {
            FORCE_POWER_FEAR | FORCE_POWER_HORROR | FORCE_POWER_INSANITY => immunity::FEAR,
            FORCE_POWER_STASIS | FORCE_POWER_MASS_STASIS => immunity::PARALYSIS,
            _ => immunity::STUN,
        }
    }

    /// `IMMUNITY_TYPE_*` values.
    pub mod immunity {
        pub const MIND_SPELLS: u8 = 1;
        pub const POISON: u8 = 2;
        pub const FEAR: u8 = 4;
        /// What TSL's program calls its mine immunity.
        pub const TRAP: u8 = 5;
        pub const PARALYSIS: u8 = 6;
        pub const STUN: u8 = 12;
        pub const SLEEP: u8 = 13;
    }

    /// The feat that makes one immune to `row`, if any.
    pub fn immunity(row: u16) -> Option<u16> {
        match row {
            FORCE_POWER_FEAR | FORCE_POWER_HORROR | FORCE_POWER_INSANITY => {
                Some(feats::FORCE_IMMUNITY_FEAR)
            }
            FORCE_POWER_STASIS | FORCE_POWER_MASS_STASIS => Some(feats::FORCE_IMMUNITY_PARALYSIS),
            FORCE_POWER_STUN
            | FORCE_POWER_FORCE_SCREAM
            | FORCE_POWER_IMPROVED_FORCE_SCREAM
            | FORCE_POWER_MASTER_FORCE_SCREAM => Some(feats::FORCE_IMMUNITY_STUN),
            _ => None,
        }
    }

    pub fn area(row: u16) -> bool {
        matches!(
            row,
            FORCE_POWER_MASS_STASIS
                | FORCE_POWER_HORROR
                | FORCE_POWER_INSANITY
                | FORCE_POWER_FORCE_SCREAM
                | FORCE_POWER_IMPROVED_FORCE_SCREAM
                | FORCE_POWER_MASTER_FORCE_SCREAM
        )
    }

    pub fn fear(row: u16) -> bool {
        matches!(
            row,
            FORCE_POWER_FEAR | FORCE_POWER_HORROR | FORCE_POWER_INSANITY
        )
    }

    pub fn droids(row: u16) -> bool {
        matches!(
            row,
            FORCE_POWER_DROID_STUN | FORCE_POWER_DROID_DISABLE | FORCE_POWER_DROID_DESTROY
        )
    }

    /// The sphere (metres) round the one aimed at that `row` also reaches:
    /// `k_sp1_generic`'s Disable Droid and Destroy Droid strike their
    /// target, then every creature within their spheres of it.
    pub fn splash(row: u16) -> Option<f32> {
        match row {
            FORCE_POWER_DROID_DISABLE => Some(rules::force::crowd::DISABLE_SPLASH_M),
            FORCE_POWER_DROID_DESTROY => Some(rules::force::crowd::DESTROY_SPLASH_M),
            _ => None,
        }
    }
}

/// The character's best row of `power`'s chain, and its level (push's for
/// pull); `None` if it knows none.
pub fn best(powers: &Powers, power: Power) -> Option<(u16, u8)> {
    let chain = if power == Power::Pull {
        Power::Push
    } else {
        power
    };
    (0..Powers::ROWS)
        .filter(|row| powers.has(*row))
        .filter_map(|row| of_row(row).map(|(known, level)| (row, known, level)))
        .filter(|(_, known, _)| *known == chain)
        .max_by_key(|(_, _, level)| *level)
        .map(|(row, _, level)| (row, level))
}

/// The row of `power`'s chain at `level` the character knows, else its best
/// (a channel keeps its level, not its row: the row it runs as).
pub fn row_at(powers: &Powers, power: Power, level: u8) -> Option<u16> {
    (0..Powers::ROWS)
        .find(|&row| powers.has(row) && of_row(row) == Some((power, level)))
        .or_else(|| best(powers, power).map(|(row, _)| row))
}

/// Whether `row` is a power only its impact script casts
/// (`Power::Scripted`): a row a Force power menu of the game offers
/// (`Library::offered`) that JA has no shape for (`of_row`), other than the
/// saber throw. Its impact script finds whom it reaches by its own shapes
/// (`GetFirstObjectInShape`), as both games' do.
pub fn scripted(library: Option<&crate::script::Library>, row: u16) -> bool {
    of_row(row).is_none()
        && library.is_some_and(|library| library.offered(row) != Menu::None)
        && !crate::combat::saber::throw::is_throw(row)
}

/// How far a scripted power (`scripted`) with the facts `talent` reaches
/// the one it is cast at (units): its `spells.2da` range through
/// `ranges.2da` (`Talent::reach_m`), as an item's spell reaches. A row whose
/// range is personal reaches as far as the crowd control does
/// (`ja::STUN_RANGE`).
pub fn scripted_reach(talent: crate::script::Talent) -> f32 {
    if talent.reach_dm == 0 {
        ja::STUN_RANGE
    } else {
        talent.reach_m() / collision::UNIT
    }
}

/// What the power button casts with `selected` (`Command::force`): the
/// power, its level and the row whose cost it pays. A player's choice
/// (`exact`) casts the row chosen, as KOTOR's quickbar casts the power
/// picked from it (Force Push though Force Wave is known); a creature's
/// casts the best row of its chain. A known row JA has no shape for is
/// `Power::Scripted` where `library` offers it (`Library::offered`).
pub fn selected(
    powers: &Powers,
    selected: u16,
    exact: bool,
    library: Option<&crate::script::Library>,
) -> Option<(Power, u8, u16)> {
    if selected == PULL {
        return best(powers, Power::Pull).map(|(row, level)| (Power::Pull, level, row));
    }
    let row = selected;
    if !powers.has(row) {
        return None;
    }
    let Some((power, level)) = of_row(row) else {
        return scripted(library, row).then_some((Power::Scripted, 1, row));
    };
    match power {
        Power::Stun => Some((Power::Stun, 1, row)),
        _ if exact => Some((power, level, row)),
        _ => best(powers, power).map(|(row, level)| (power, level, row)),
    }
}

/// What a player's power button can select (`Command::force`): every
/// power row it knows that it can cast, each chain's ranks together and in
/// rank order (the chains as `Power` lists them, pull after push when push
/// is known), the rows of one rank by row; then the powers only their
/// impact scripts cast (`Power::Scripted`), by row; last the saber throw
/// it knows, which the button throws (`motion::saber`). Every row of the
/// table counts, TSL's past 255 too (Breath Control is row 270), but a row
/// the level's table shows no power at (`Library::lacks_power`), which the
/// bar and the wheel could only show blank.
pub fn choices(powers: &Powers, library: Option<&crate::script::Library>) -> Vec<u16> {
    let mut rows: Vec<(Power, u8, u16)> = (0..Powers::ROWS)
        .filter(|row| *row != PULL)
        .filter(|row| !library.is_some_and(|library| library.lacks_power(*row)))
        .filter_map(|row| {
            selected(powers, row, true, library).map(|(power, level, _)| (power, level, row))
        })
        .collect();
    if best(powers, Power::Push).is_some() {
        rows.push((Power::Pull, 1, PULL));
    }
    rows.sort();
    let throw = crate::combat::saber::throw::level(powers).map(|(_, row)| row);
    rows.into_iter().map(|(.., row)| row).chain(throw).collect()
}

/// A power channelled while its button is held (JA's grip, lightning,
/// drain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel {
    pub power: Power,
    pub level: u8,
    /// When it runs out unless held on (sim ms), and its next tick.
    pub until: u64,
    pub next_tick: u64,
    /// When it started (sim ms): costs past its first run (C12,
    /// `ja::CHANNEL_MS`), grip's lift (`ja::GRIP_LIFT_EVERY_MS`) and its one
    /// crush (`ja::GRIP_CRUSH_MS`).
    pub from: u64,
    /// The grip's victim.
    pub target: Option<SimId>,
}

impl Channel {
    const BLANK: Channel = Channel {
        power: Power::Push,
        level: 1,
        until: 0,
        next_tick: 0,
        from: 0,
        target: None,
    };

    fn encode(&self, out: &mut Writer) {
        out.u8(self.power as u8);
        out.u8(self.level);
        out.u64(self.until);
        out.u64(self.next_tick);
        out.u64(self.from);
        write_fixed(out, self.target.as_ref(), &SimId(0), encode_id);
    }

    fn decode(input: &mut Reader) -> Result<Channel, DecodeError> {
        let at = input.at();
        let power = POWERS
            .get(usize::from(input.u8()?))
            .copied()
            .ok_or(input.invalid("force power", at))?;
        let at = input.at();
        let level = input.u8()?;
        if !(1..=3).contains(&level) {
            return Err(input.invalid("channel level", at));
        }
        Ok(Channel {
            power,
            level,
            until: input.u64()?,
            next_tick: input.u64()?,
            from: input.u64()?,
            target: read_fixed(input, &SimId(0), decode_id, "grip target flag")?,
        })
    }
}

/// A hand held out (JA's `forceHandExtend`): what it plays on the torso
/// while it lasts. Grip, lightning and drain hold theirs while they run
/// (`Channel`); these are the timed ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// Push, and JA's mind trick and our crowd control, which use its hand.
    Push,
    Pull,
    /// A grip's victim at the crush (the whole body when off the ground).
    Choke,
    /// A power only its impact script casts (`Power::Scripted`): the hand is
    /// busy, and the body shows the row's own cast clips
    /// (`Fighter::acting`, `ForceState::cast_loop`) instead of a JA gesture.
    Cast,
}

const GESTURES: [Gesture; 4] = [Gesture::Push, Gesture::Pull, Gesture::Choke, Gesture::Cast];

/// What a character's Force is doing (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ForceState {
    /// Sim ms at which the pool next regains a point.
    pub regen_at: u64,
    /// TSL's regeneration's part of a hundredth not yet gained, in
    /// thousandths (`regenerate_tsl`, the executable's accumulator).
    pub fp_carry: i32,
    /// The force buttons held last step: instant powers fire on a press.
    pub held: u16,
    /// Running toggles: until when (sim ms; 0 off), and the level cast.
    pub speed_until: u64,
    pub protect_until: u64,
    pub absorb_until: u64,
    pub trick_until: u64,
    pub speed_level: u8,
    pub protect_level: u8,
    pub absorb_level: u8,
    pub trick_level: u8,
    /// A channel under way.
    pub channel: Option<Channel>,
    /// A timed hand, and until when (sim ms).
    pub hand: Option<(Gesture, u64)>,
    /// When each may start again (sim ms): push and pull (shared), grip,
    /// lightning, drain.
    pub push_pull_ready: u64,
    pub grip_ready: u64,
    pub lightning_ready: u64,
    pub drain_ready: u64,
    /// Effects on this character: gripped by whom; stunned (C11's hold),
    /// then immune until; what holds it (fear's ends on damage; the views
    /// show each its way); confused (a creature's mind trick: no enemy)
    /// until.
    pub gripped_by: Option<SimId>,
    pub stunned_until: u64,
    pub immune_until: u64,
    pub hold: Hold,
    pub confused_until: u64,
    /// Seats whose players do not see this character (JA's mind trick on
    /// players: presentation; the view hides it), one bit per seat.
    pub tricked: u64,
    /// A force jump is active (past a normal jump's height, until landing:
    /// `motion::force_jump`), and when it next pays.
    pub jumping: bool,
    pub jump_paid_at: u64,
    /// Rage (TSL's Fury): until when and at what level, when it next costs
    /// health, and until when its recovery lasts after it.
    pub rage_until: u64,
    pub rage_level: u8,
    pub rage_paid_at: u64,
    pub recover_until: u64,
    /// Sight: until when (the view shows others through walls; it breaks
    /// mind tricks on its user).
    pub sight_until: u64,
    /// The KOTOR state holding it (`effects::state`; 0 for none): the
    /// highest of its running state effects as last settled
    /// (`effects::settle`), and until when that effect runs. No cap, no
    /// immune second: it lasts as long as its effect.
    pub state: u8,
    pub state_until: u64,
    /// When this character last uncovered its ears after a Force Scream
    /// (sim ms; 0 for never): `cover_ears` refuses another covering until
    /// `ja::EARS_IMMUNE_MS` after it.
    pub ears_until: u64,
    /// The looped clip of a scripted power's cast (`Gesture::Cast`), the
    /// time its conjuring ends and the time its cast ends (sim ms): the body
    /// plays the row's conjure clip first, then this clip until the cast
    /// ends.
    pub cast_loop: Option<(u16, u64, u64)>,
    /// The saving throw the running hold rolls each round to break free
    /// (`None`: it rolls none), when it next rolls (sim ms), and how many
    /// rolls it failed so far.
    pub save: Option<HoldSave>,
    pub save_due: u64,
    pub rolls: u8,
    /// How many recent holds this character had, and until when the last
    /// one counts as recent (sim ms).
    pub recent: u8,
    pub recent_until: u64,
}

/// The saving throw that first allowed a hold: `Sheet::save`'s index (0
/// Fortitude, 1 Reflex, 2 Will) and its DC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldSave {
    pub index: u8,
    pub dc: i32,
}

/// One roll to break free (`ForceState::roll_free`): the save, the d20,
/// the total it added (the save and the struggle bonus), and whether it
/// broke free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Struggled {
    pub save: HoldSave,
    pub roll: i32,
    pub total: i32,
    pub saved: bool,
}

/// What kind of hold `ForceState::hold` applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldKind {
    /// A combat hold: its own duration, shorter when repeated, and the save
    /// it rolls each round (`None`: it allowed no save).
    Combat(Option<HoldSave>),
    /// A story script's or a friend's hold: today's synthetic round, no
    /// saves, not counted as recent.
    Story,
    /// kjk's own powers where no script runs: a synthetic round, no saves,
    /// counted as recent.
    Own,
}

impl ForceState {
    /// Holds the character for `ms` under C11 (see `HoldKind`), then
    /// immunity (`ja::IMMUNE_MS`). Nothing while immune, nor after
    /// `ja::MOST_RECENT` recent holds. Whether it took hold.
    pub fn hold(&mut self, now: u64, ms: u64, kind: HoldKind) -> bool {
        if self.immune_until > now {
            return false;
        }
        let recent = if self.recent_until > now {
            self.recent
        } else {
            0
        };
        let (length, save) = match kind {
            HoldKind::Story | HoldKind::Own => (ms.min(ja::STUN_MS), None),
            HoldKind::Combat(save) => {
                if recent >= ja::MOST_RECENT {
                    return false;
                }
                let full = ms.min(ja::LONGEST_MS);
                let percent = match recent {
                    0 => 100,
                    1 => ja::SECOND_PERCENT,
                    _ => ja::THIRD_PERCENT,
                };
                ((full * percent / 100).max(full.min(ja::ROUND_MS)), save)
            }
        };
        self.stunned_until = now + length;
        self.immune_until = self.stunned_until + ja::IMMUNE_MS;
        self.save = save;
        self.save_due = now + ja::ROUND_MS;
        self.rolls = 0;
        if kind != HoldKind::Story {
            self.recent = recent.saturating_add(1);
            self.recent_until = self.stunned_until + ja::RECENT_MS;
        }
        true
    }

    /// The saving throw due now to break free of the running hold, and its
    /// struggle bonus (`ja::STRUGGLE_BONUS` for each roll it failed).
    pub fn due(&self, now: u64) -> Option<(HoldSave, i32)> {
        let save = self.save?;
        (self.stunned_until > now && now >= self.save_due)
            .then_some((save, i32::from(self.rolls) * ja::STRUGGLE_BONUS))
    }

    /// A due roll's outcome: a save that succeeded ends the hold now; one
    /// that failed waits another round (`ja::ROUND_MS`).
    pub fn struggle(&mut self, now: u64, saved: bool) {
        if saved {
            self.release(now);
        } else {
            self.rolls = self.rolls.saturating_add(1);
            self.save_due += ja::ROUND_MS;
        }
    }

    /// Rolls the save due now, if any (`due`): `d20()` plus `save(index)`
    /// plus the struggle bonus against its DC (`combat::check`), then
    /// `struggle`. What was rolled.
    pub fn roll_free(
        &mut self,
        now: u64,
        d20: impl FnOnce() -> i32,
        save: impl FnOnce(usize) -> i32,
    ) -> Option<Struggled> {
        let (hold, bonus) = self.due(now)?;
        let roll = d20();
        let total = save(usize::from(hold.index)) + bonus;
        let saved = crate::combat::check(roll, total, hold.dc);
        self.struggle(now, saved);
        Some(Struggled {
            save: hold,
            roll,
            total,
            saved,
        })
    }

    /// Whether a combat hold is refused now for the recent holds alone
    /// (`ja::MOST_RECENT`), not for the immunity after a hold.
    pub fn weary(&self, now: u64) -> bool {
        self.immune_until <= now && self.recent_until > now && self.recent >= ja::MOST_RECENT
    }

    /// Ends the running hold now (a fear's on damage, a save that broke
    /// free): no more rolls, immunity from now, and recent from now.
    pub fn release(&mut self, now: u64) {
        if self.stunned_until <= now {
            return;
        }
        if self.recent_until == self.stunned_until + ja::RECENT_MS {
            self.recent_until = now + ja::RECENT_MS;
        }
        self.stunned_until = now;
        self.immune_until = now + ja::IMMUNE_MS;
        self.save = None;
    }

    /// Covers the ears after a failed save against Force Scream: held for
    /// `ja::EARS_MS` (`Hold::Ears`). Nothing while another hold runs, nor
    /// until `ja::EARS_IMMUNE_MS` after the last covering ended (`deaf`).
    /// Whether it took hold.
    pub fn cover_ears(&mut self, now: u64) -> bool {
        if self.deaf(now) || self.stunned_until > now {
            return false;
        }
        self.stunned_until = now + ja::EARS_MS;
        self.hold = Hold::Ears;
        self.save = None;
        self.rolls = 0;
        self.ears_until = self.stunned_until;
        true
    }

    /// Whether a Force Scream cannot make this character cover its ears
    /// now: it covers them, or uncovered them less than
    /// `ja::EARS_IMMUNE_MS` ago.
    pub fn deaf(&self, now: u64) -> bool {
        self.ears_until > 0 && now < self.ears_until + ja::EARS_IMMUNE_MS
    }

    /// Since when this character covers its ears, while it does.
    pub fn covering_ears(&self, now: u64) -> Option<u64> {
        (self.hold == Hold::Ears && self.stunned_until > now)
            .then(|| self.stunned_until.saturating_sub(ja::EARS_MS))
    }

    /// When the running hold began (sim ms). `hold` puts the first saving
    /// throw one round after the hold begins, and each failed throw puts
    /// the next one a round later, so the start is that many rounds before
    /// the next throw. Covering the ears starts `ja::EARS_MS` before it
    /// ends.
    pub fn held_since(&self) -> u64 {
        if self.hold == Hold::Ears {
            return self.stunned_until.saturating_sub(ja::EARS_MS);
        }
        let rounds = u64::from(self.rolls) + 1;
        self.save_due.saturating_sub(rounds * ja::ROUND_MS)
    }

    /// Whether a power runs that pauses regeneration (JA: any active power
    /// but drain alone).
    pub fn pausing(&self, now: u64) -> bool {
        self.speed_until > now
            || self.protect_until > now
            || self.absorb_until > now
            || self.trick_until > now
            || self.rage_until > now
            || self.sight_until > now
            || self.jumping
            || self
                .channel
                .is_some_and(|channel| channel.power != Power::Drain)
    }

    /// Whether a hand is out: no new hand power starts meanwhile.
    pub fn hand_out(&self, now: u64) -> bool {
        self.hand.is_some_and(|(_, until)| until > now) || self.channel.is_some()
    }

    /// Held still: by C11's hold, or by a KOTOR state that holds
    /// (`effects::state::holds`).
    pub fn stunned(&self, now: u64) -> bool {
        self.stunned_until > now
            || (self.state_until > now && crate::combat::effects::state::holds(self.state))
    }

    /// The absorb level against an incoming power, 0 when it is off.
    pub fn absorbing(&self, now: u64) -> u8 {
        if self.absorb_until > now {
            self.absorb_level
        } else {
            0
        }
    }

    /// Ends the running toggles, the channel and the hand (a death).
    pub fn stop(&mut self) {
        self.speed_until = 0;
        self.protect_until = 0;
        self.absorb_until = 0;
        self.trick_until = 0;
        self.channel = None;
        self.hand = None;
        self.cast_loop = None;
        self.tricked = 0;
        self.jumping = false;
        self.rage_until = 0;
        self.sight_until = 0;
    }

    /// Whether rage runs, and at what level (0 when it does not).
    pub fn raging(&self, now: u64) -> u8 {
        if self.rage_until > now {
            self.rage_level
        } else {
            0
        }
    }

    /// The fire time's factor (the weapon step's fire time): `ja::RAGE_REFIRE` raging,
    /// `ja::RECOVER_REFIRE` in rage's recovery.
    pub fn refire(&self, now: u64) -> f32 {
        if self.rage_until > now {
            ja::RAGE_REFIRE
        } else if self.recover_until > now {
            ja::RECOVER_REFIRE
        } else {
            1.0
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u64(self.regen_at);
        out.u32(self.fp_carry as u32);
        out.u16(self.held);
        for time in [
            self.speed_until,
            self.protect_until,
            self.absorb_until,
            self.trick_until,
        ] {
            out.u64(time);
        }
        out.bytes(&[
            self.speed_level,
            self.protect_level,
            self.absorb_level,
            self.trick_level,
        ]);
        write_fixed(out, self.channel.as_ref(), &Channel::BLANK, Channel::encode);
        write_fixed(
            out,
            self.hand.as_ref(),
            &(Gesture::Push, 0),
            |(gesture, until), out| {
                out.u8(*gesture as u8);
                out.u64(*until);
            },
        );
        for time in [
            self.push_pull_ready,
            self.grip_ready,
            self.lightning_ready,
            self.drain_ready,
        ] {
            out.u64(time);
        }
        write_fixed(out, self.gripped_by.as_ref(), &SimId(0), encode_id);
        out.u64(self.stunned_until);
        out.u64(self.immune_until);
        out.u8(self.hold.code());
        out.u64(self.confused_until);
        out.u64(self.tricked);
        out.u8(u8::from(self.jumping));
        out.u64(self.jump_paid_at);
        out.u64(self.rage_until);
        out.u8(self.rage_level);
        out.u64(self.rage_paid_at);
        out.u64(self.recover_until);
        out.u64(self.sight_until);
        out.u8(self.state);
        out.u64(self.state_until);
        out.u64(self.ears_until);
        write_fixed(
            out,
            self.cast_loop.as_ref(),
            &(0, 0, 0),
            |(clip, from, until), out| {
                out.u16(*clip);
                out.u64(*from);
                out.u64(*until);
            },
        );
        write_fixed(
            out,
            self.save.as_ref(),
            &HoldSave { index: 0, dc: 0 },
            |save, out| {
                out.u8(save.index);
                out.u32(save.dc as u32);
            },
        );
        out.u64(self.save_due);
        out.u8(self.rolls);
        out.u8(self.recent);
        out.u64(self.recent_until);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<ForceState, DecodeError> {
        let regen_at = input.u64()?;
        let fp_carry = input.u32()? as i32;
        let held = input.u16()?;
        let [speed_until, protect_until, absorb_until, trick_until] =
            [input.u64()?, input.u64()?, input.u64()?, input.u64()?];
        let [speed_level, protect_level, absorb_level, trick_level] = input.array()?;
        let channel = read_fixed(input, &Channel::BLANK, Channel::decode, "channel flag")?;
        let hand = read_fixed(
            input,
            &(Gesture::Push, 0),
            |input| {
                let at = input.at();
                let gesture = GESTURES
                    .get(usize::from(input.u8()?))
                    .copied()
                    .ok_or(input.invalid("gesture", at))?;
                Ok((gesture, input.u64()?))
            },
            "hand flag",
        )?;
        let [push_pull_ready, grip_ready, lightning_ready, drain_ready] =
            [input.u64()?, input.u64()?, input.u64()?, input.u64()?];
        let gripped_by = read_fixed(input, &SimId(0), decode_id, "grip flag")?;
        let stunned_until = input.u64()?;
        let immune_until = input.u64()?;
        let at = input.at();
        let hold = Hold::of_code(input.u8()?).ok_or(input.invalid("hold", at))?;
        let confused_until = input.u64()?;
        let tricked = input.u64()?;
        let at = input.at();
        let jumping = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("force jump flag", at)),
        };
        let mut state = ForceState {
            regen_at,
            fp_carry,
            held,
            speed_until,
            protect_until,
            absorb_until,
            trick_until,
            speed_level,
            protect_level,
            absorb_level,
            trick_level,
            channel,
            hand,
            push_pull_ready,
            grip_ready,
            lightning_ready,
            drain_ready,
            gripped_by,
            stunned_until,
            immune_until,
            hold,
            confused_until,
            tricked,
            jumping,
            jump_paid_at: input.u64()?,
            rage_until: input.u64()?,
            rage_level: input.u8()?,
            rage_paid_at: input.u64()?,
            recover_until: input.u64()?,
            sight_until: input.u64()?,
            state: input.u8()?,
            state_until: input.u64()?,
            ears_until: if input.since(crate::since::snapshot::FEAT_FORCE_SCREAM_EARS) {
                input.u64()?
            } else {
                0
            },
            cast_loop: if input.since(crate::since::snapshot::FEAT_POWER_ROWS) {
                read_fixed(
                    input,
                    &(0, 0, 0),
                    |input| Ok((input.u16()?, input.u64()?, input.u64()?)),
                    "cast loop flag",
                )?
            } else {
                None
            },
            ..ForceState::default()
        };
        if input.since(crate::since::snapshot::FEAT_STUN_SAVES) {
            state.save = read_fixed(
                input,
                &HoldSave { index: 0, dc: 0 },
                |input| {
                    Ok(HoldSave {
                        index: input.u8()?,
                        dc: input.u32()? as i32,
                    })
                },
                "hold save flag",
            )?;
            state.save_due = input.u64()?;
            state.rolls = input.u8()?;
            state.recent = input.u8()?;
            state.recent_until = input.u64()?;
        }
        Ok(state)
    }
}

/// `Power`s in their numbering (`as u8`).
const POWERS: [Power; 13] = [
    Power::Push,
    Power::Pull,
    Power::Lightning,
    Power::Drain,
    Power::Grip,
    Power::Speed,
    Power::Heal,
    Power::Protect,
    Power::Absorb,
    Power::MindTrick,
    Power::Stun,
    Power::Rage,
    Power::Sight,
];

/// How long regaining one point takes (ms) for a character knowing `feats`:
/// the whole pool in the book's `force.pool.refill_ms` (C13). Regenerate
/// Force Points adds what it adds in KOTOR 2 on top: `REGENERATE_FEAT_RATE`
/// of the pool every `force.k1_regeneration.force_time_s`.
pub fn regen_every(feats: &crate::combat::sheet::Feats, max_fp: u16) -> u64 {
    let max_fp = max_fp.max(1);
    if !feats.has(REGENERATE_FEAT) {
        return (rules::force::pool::REFILL_MS / u64::from(max_fp)).max(1);
    }
    let pools_per_ms = 1.0 / rules::force::pool::REFILL_MS as f32
        + REGENERATE_FEAT_RATE / (rules::force::k1_regeneration::FORCE_TIME_S * 1000.0);
    ((1.0 / (pools_per_ms * f32::from(max_fp))) as u64).max(1)
}

/// TSL's feat that adds to Force point regeneration (`feat.2da` 143,
/// `REGENERATE_FORCE_POINTS`), and how much.
pub const REGENERATE_FEAT: u16 = crate::combat::sheet::feats::REGENERATE_FORCE_POINTS;
pub const REGENERATE_FEAT_RATE: f32 = rules::force::tsl_regeneration::FORCE_FEAT_RATE;

/// TSL's Force point regeneration for one step of `msec` (its executable's
/// `HandleFPRegeneration`, which replaces C13 in TSL): a rate of the
/// table's out-of-combat base, or in a real fight its in-combat base (Niman
/// and Force Affinity a share of the other, `forms::fp_base`); plus its
/// Wisdom modifier times the table's bonus, `REGENERATE_FEAT_RATE` with
/// `REGENERATE_FEAT`, each regeneration effect's percent (`effect_percent`)
/// and Force Focus's (`forms::fp_bonus`); then the room's Force rating
/// (`rating`) takes that share off for the dark or adds it for the light
/// (the book's `force.tsl_regeneration` alignments). The pool is the
/// executable's `GetMaxForcePoints` (`Sheet::force_points`, the Wisdom
/// modifier for each level counted). The pool and its temporary points over it (`over`,
/// whole; the map reads the executable's `maxFP + stats[+0x18a]`, that
/// field unnamed: [verify] whether it is the temporary points) fill at that
/// rate a `table.time` seconds, by hundredths kept in
/// an accumulator (`ForceState::fp_carry`); within 0 and the pool and
/// those points. A negative rate drains it.
#[allow(clippy::too_many_arguments)]
pub fn regenerate_tsl(
    state: &mut ForceState,
    fp: &mut i32,
    sheet: &crate::combat::Sheet,
    table: crate::script::FpRegeneration,
    (fighting, effect_percent, rating, over): (bool, i32, i32, i32),
    msec: u32,
) {
    use crate::combat::forms;
    use crate::combat::sheet::Ability;
    const DARK: u8 = rules::force::tsl_regeneration::DARK_BELOW;
    const LIGHT: u8 = rules::force::tsl_regeneration::LIGHT_FROM;
    let mut rate = if fighting {
        forms::fp_base(sheet, table.out_of_combat, table.in_combat)
    } else {
        table.out_of_combat
    };
    rate += table.wis_bonus * sheet.modifier(Ability::Wisdom) as f32;
    if sheet.feats.has(REGENERATE_FEAT) {
        rate += REGENERATE_FEAT_RATE;
    }
    rate += effect_percent as f32 / 100.0;
    rate += forms::fp_bonus(sheet);
    let shift = rating as f32 * rate / 100.0;
    match sheet.good_evil {
        ..DARK => rate -= shift,
        LIGHT.. => rate += shift,
        _ => {}
    }
    let most = i32::from(sheet.force_points()) + over.max(0);
    let per_second = rate / table.time.max(0.0001) * most as f32;
    let gained = per_second * 100.0 * msec as f32 / 1000.0 + state.fp_carry as f32 / 1000.0;
    let whole = gained.trunc();
    state.fp_carry = ((gained - whole) * 1000.0).round() as i32;
    *fp = (*fp + whole as i32).clamp(0, most * 100);
}

/// One step of regeneration (C13, JA's power upkeep): a point
/// each `regen_every` for a character knowing `feats`, up to the maximum;
/// while `paused` (or full) the clock starts over, so it resumes a period
/// after the pause ends.
pub fn regenerate(
    state: &mut ForceState,
    fp: &mut i32,
    (feats, max_fp): (&crate::combat::sheet::Feats, u16),
    paused: bool,
    now: u64,
) {
    let every = regen_every(feats, max_fp);
    let max = i32::from(max_fp) * 100;
    if paused || *fp >= max {
        state.regen_at = state.regen_at.max(now + every);
        return;
    }
    while state.regen_at <= now && *fp < max {
        *fp = (*fp + 100).min(max);
        state.regen_at += every;
    }
}

/// A channel's cost this step (C12, hundredths of an FP): nothing for its
/// first run (`ja::CHANNEL_MS`: the cast paid it), then `cost` per second
/// held.
pub fn channel_cost(cost: i32, from: u64, now: u64, msec: u32) -> i32 {
    if now < from + ja::CHANNEL_MS {
        0
    } else {
        (cost * i32::try_from(msec).unwrap_or(0) / 1000).max(1)
    }
}

/// Protect at `level` against a hit of `damage` hundredths of health on one
/// with `max_hp` most health and `fp` of its `max_fp` pool (hundredths):
/// the hundredths it prevents and the hundredths of pool it pays. JA counts
/// in its points (1% of the most): a share of the hit up to the level's cap
/// is stopped at the level's rate of pool, and what the pool cannot pay
/// gets through; the part above the cap gets through free (QUIRKS.md
/// Q-0033).
pub fn protect(level: u8, damage: i32, max_hp: u16, fp: i32, max_fp: u16) -> (i32, i32) {
    let index = usize::from(level.clamp(1, 3) - 1);
    let max_hp = i64::from(max_hp.max(1));
    let max_fp = i64::from(max_fp.max(1));
    let damage = i64::from(damage);
    // Hundredths of JA points.
    let hit = damage * 100 / max_hp;
    let counted = hit.min(ja::PROTECT_CAP[index] * 100);
    let owed = counted * ja::PROTECT_RATE[index] / 1000;
    let stopped = counted * ja::PROTECT_SHARE[index] / 1000;
    let pool = i64::from(fp) * 100 / max_fp;
    let paid = owed.min(pool);
    let prevented = stopped - (owed - paid);
    let prevented = (prevented.max(0) * max_hp / 100).min(damage);
    (prevented as i32, (paid * max_fp / 100) as i32)
}

/// JA's numbers for the powers (I23 section 4), from the book's chapter
/// `force` (`data/rules/force.toml`).
pub mod ja {
    use rules::force::absorb;
    use rules::force::channel;
    use rules::force::crowd;
    use rules::force::drain;
    use rules::force::grip;
    use rules::force::heal;
    use rules::force::lightning;
    use rules::force::mind_trick;
    use rules::force::protect;
    use rules::force::push;
    use rules::force::rage;
    use rules::force::sight;
    use rules::force::speed;

    pub const TICK_MS: u64 = channel::TICK_MS;
    pub const CHANNEL_MS: u64 = channel::RUN_MS;
    pub const DELAY_MS: [u64; 2] = channel::DELAY_MS;
    pub const LINE_RANGE: f32 = channel::LINE_RANGE;
    pub const CONE_DOT: f32 = channel::CONE_DOT;
    pub const IMPACT_EVERY_MS: u64 = channel::IMPACT_EVERY_MS;
    pub const LIGHTNING_RADIUS: f32 = lightning::RADIUS;
    pub const LIGHTNING_POINTS: i32 = lightning::POINTS;
    pub const LIGHTNING_EXTRA: u8 = lightning::EXTRA;
    pub const DRAIN_RADIUS: f32 = drain::RADIUS;
    pub const DRAIN: [i32; 3] = drain::POINTS;
    pub const DRAIN_CASTER_WAIT_MS: u64 = drain::CASTER_WAIT_MS;
    pub const DRAIN_VICTIM_WAIT_MS: u64 = drain::VICTIM_WAIT_MS;
    pub const GRIP_RANGE: f32 = grip::RANGE;
    pub const GRIP_POINTS: i32 = grip::POINTS;
    pub const GRIP_SQUEEZE_MS: u64 = grip::SQUEEZE_EVERY_MS;
    pub const GRIP_HOLD_MS: [u64; 3] = grip::HOLD_MS;
    pub const GRIP_CRUSH_MS: u64 = grip::CRUSH_MS;
    pub const GRIP_CRUSH: [i32; 3] = grip::CRUSH;
    pub const GRIP_CRUSH_HAND_MS: u64 = grip::CRUSH_HAND_MS;
    pub const GRIP_FRONT_DOT: f32 = grip::FRONT_DOT;
    pub const GRIP_LIFT_EVERY_MS: u64 = grip::LIFT_EVERY_MS;
    pub const GRIP_LIFT: f32 = grip::LIFT;
    pub const GRIP_CARRY_AHEAD: f32 = grip::CARRY_AHEAD;
    pub const GRIP_CARRY_UP: f32 = grip::CARRY_UP;
    pub const GRIP_CARRY_WITHIN: [f32; 4] = grip::CARRY_WITHIN;
    pub const GRIP_CARRY_SPEEDS: [f32; 4] = grip::CARRY_SPEEDS;
    pub const GRIP_CARRY_BEYOND: f32 = grip::CARRY_BEYOND;
    pub const GRIP_COOLDOWN_MS: u64 = grip::COOLDOWN_MS;
    pub const GRIP_BROKEN_MS: u64 = grip::BROKEN_MS;
    pub const GRIPPING_SCALE: f32 = grip::GRIPPING_SCALE;
    pub const GRIPPED_SCALE: [f32; 2] = grip::GRIPPED_SCALE;
    pub const PUSH_PULL_AIMED: f32 = push::AIMED_REACH;
    pub const PUSH_PULL_RANGE: f32 = push::AREA_REACH;
    pub const PUSH_PULL_FIELD: [f32; 2] = push::FIELD;
    pub const PUSH_PULL_FRONT: f32 = push::FRONT_DOT;
    pub const PUSH_PULL_STRENGTH: f32 = push::STRENGTH;
    pub const SHOVE_FALLOFF: f32 = push::FALLOFF;
    pub const SHOVE_MIN: f32 = push::LEAST_SHOVE;
    pub const SHOVE_LIFT: f32 = push::LEAST_LIFT;
    pub const PUSH_HAND_MS: u64 = push::PUSH_HAND_MS;
    pub const PULL_HAND_MS: u64 = push::PULL_HAND_MS;
    pub const PUSH_COOLDOWN_MS: u64 = push::PUSH_COOLDOWN_MS;
    pub const PULL_COOLDOWN_MS: u64 = push::PULL_COOLDOWN_MS;
    pub const KNOCKDOWN_MS: u64 = push::KNOCKDOWN_MS;
    pub const KNOCKDOWN_REACH: f32 = push::KNOCKDOWN_REACH;
    pub const COUNTERED: [f32; 3] = push::COUNTERED;
    pub const WHIRLWIND_DIVISOR: u16 = rules::force::push::whirlwind::LEVEL_DIVISOR;
    pub const WHIRLWIND_LEAST: u16 = rules::force::push::whirlwind::LEAST_DAMAGE;
    pub const ABSORB_THROW: i32 = absorb::THROW_GAIN;
    pub const ABSORB_TICK: i32 = absorb::TICK_GAIN;
    pub const SPEED_SCALE: f32 = speed::SCALE;
    pub const SPEED_MS: [u64; 3] = speed::LENGTH_MS;
    pub const TOGGLE_MIN_MS: u64 = rules::force::casting::TOGGLE_MIN_MS;
    pub const GUARD_MS: u64 = protect::LENGTH_MS;
    pub const PROTECT_SHARE: [i64; 3] = protect::SHARE;
    pub const PROTECT_RATE: [i64; 3] = protect::RATE;
    pub const PROTECT_CAP: [i64; 3] = protect::CAP;
    pub const HEAL_RANGE: [f32; 3] = heal::REACH;
    pub const HEAL: [i32; 3] = heal::POINTS;
    pub const TRICK_AIMED: f32 = mind_trick::AIMED_REACH;
    pub const TRICK_AREA: f32 = mind_trick::AREA_REACH;
    pub const TRICK_FIELD: f32 = mind_trick::AREA_FIELD;
    pub const TRICK_MS: [u64; 3] = mind_trick::LENGTH_MS;
    pub const CONFUSE_MS: [u64; 3] = mind_trick::CONFUSE_MS;
    /// The hand a cast puts out (mind trick's, and every KOTOR power's).
    pub const TRICK_HAND_MS: u64 = rules::force::casting::HAND_MS;
    pub const STUN_RANGE: f32 = crowd::REACH;
    pub const STUN_FIELD: f32 = crowd::AREA_FIELD;
    pub const STUN_MS: u64 = crowd::HOLD_MS;
    pub const IMMUNE_MS: u64 = crowd::IMMUNE_MS;
    pub const ROUND_MS: u64 = crowd::ROUND_MS;
    pub const STRUGGLE_BONUS: i32 = crowd::STRUGGLE_BONUS;
    pub const LONGEST_MS: u64 = crowd::LONGEST_MS;
    pub const RECENT_MS: u64 = crowd::RECENT_MS;
    pub const SECOND_PERCENT: u64 = crowd::SECOND_PERCENT;
    pub const THIRD_PERCENT: u64 = crowd::THIRD_PERCENT;
    pub const MOST_RECENT: u8 = crowd::MOST_RECENT;
    pub const EARS_MS: u64 = crowd::EARS_MS;
    pub const EARS_IMMUNE_MS: u64 = crowd::EARS_IMMUNE_MS;
    pub const RAGE_MS: [u64; 3] = rage::LENGTH_MS;
    pub const RAGE_HP_EVERY_MS: [u64; 3] = rage::HP_EVERY_MS;
    pub const RAGE_HP: i32 = rage::HP;
    pub const RAGE_LEAST: i32 = rage::LEAST;
    pub const RAGE_FLOOR_HP: i32 = rage::FLOOR_HP;
    pub const RECOVER_MS: u64 = rage::RECOVER_MS;
    pub const RAGE_SPEED: f32 = rage::SPEED;
    pub const RECOVER_SPEED: f32 = rage::RECOVER_SPEED;
    pub const RAGE_REFIRE: f32 = rage::REFIRE;
    pub const RECOVER_REFIRE: f32 = rage::RECOVER_REFIRE;
    pub const SIGHT_MS: [u64; 3] = sight::LENGTH_MS;
}
