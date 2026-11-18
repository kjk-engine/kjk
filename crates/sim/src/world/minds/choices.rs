//! What a mind needs to choose a Force power, an item or a grenade of its
//! own (`combat::choose`): the options it has now, what it sees of its
//! target and its allies, the members of each side of the party's fight,
//! and the line the choice log writes.

use collision::UNIT;

use super::Seen;
use crate::combat::Fighter;
use crate::combat::ai::{self, Cast};
use crate::combat::choose::{self, Choice, Kind, Member, Option_, Scene, Target};
use crate::combat::force::{self, Power};
use crate::combat::sheet::Sheet;
use crate::combat::{grenades, items};
use crate::math::{self, Vec3};
use crate::snapshot::SimId;

/// The farthest a mind throws a grenade (units).
const THROW_FARTHEST: f32 = rules::minds::items::GRENADE_FARTHEST_M / UNIT;

/// What a mind reads of itself to list its options.
pub(super) struct Chooser<'a> {
    pub sheet: &'a Sheet,
    pub fighter: &'a Fighter,
    /// A party member the AI controls.
    pub companion: bool,
    pub now: u64,
    /// The distance to its target (units), if it has one.
    pub target: Option<f32>,
    /// What a power costs it (hundredths of a Force point).
    pub cost: &'a dyn Fn(u16) -> Option<i32>,
    pub scripted: ai::Scripted<'a>,
    /// The talent facts' `hostile` for a row, where the scripts' library
    /// has the row.
    pub hostile: &'a dyn Fn(u16) -> Option<bool>,
}

impl Chooser<'_> {
    /// Every option it has now, each with the cast that uses it.
    pub fn options(&self) -> Vec<(Option_, Cast)> {
        let mut options = self.powers();
        options.extend(self.items());
        options
    }

    /// The Force powers it can use now. Each is the highest it knows of
    /// its line, it has the Force points for it, the power's own wait is
    /// over, and a power used on an enemy reaches its target.
    fn powers(&self) -> Vec<(Option_, Cast)> {
        let powers = &self.sheet.powers;
        powers
            .rows()
            .filter(|&row| match force::of_row(row) {
                Some((power, level)) => {
                    force::best(powers, power).is_some_and(|(_, best)| best == level)
                        && self.power_ready(power)
                }
                None => true,
            })
            .filter_map(|row| {
                let (kind, droids_only) = choose::kind_of(row, (self.hostile)(row))?;
                let at_enemy = matches!(kind, Kind::Disable | Kind::Damage | Kind::AreaDamage);
                let reach = ai::power_reach(row, self.scripted);
                if at_enemy && self.target.is_none_or(|distance| distance > reach) {
                    return None;
                }
                let cast = ai::power_cast(row, self.sheet, self.fighter, self.cost, self.scripted)?;
                let option = Option_ {
                    choice: Choice::Power(row),
                    kind,
                    droids_only,
                    running: force::of_row(row).is_some_and(|(power, _)| self.running(power)),
                };
                Some((option, cast))
            })
            .collect()
    }

    /// Whether the wait after its last use of `power` is over.
    fn power_ready(&self, power: Power) -> bool {
        let force = &self.fighter.force;
        let ready = match power {
            Power::Grip => force.grip_ready,
            Power::Lightning => force.lightning_ready,
            Power::Drain => force.drain_ready,
            Power::Push | Power::Pull => force.push_pull_ready,
            _ => 0,
        };
        ready <= self.now
    }

    /// Whether the buff `power` already runs on it.
    fn running(&self, power: Power) -> bool {
        let (force, effects, now) = (&self.fighter.force, &self.fighter.effects, self.now);
        match power {
            Power::Speed => force.speed_until > now,
            Power::Protect => force.protect_until > now,
            Power::Absorb => force.absorb_until > now,
            Power::Valor => effects.valor(now) > 0,
            Power::Resist => effects.resisting(now) > 0,
            // A rage that ended leaves the character tired for a while, and
            // a new one cannot start until then.
            Power::Rage => force.rage_until > now || force.recover_until > now,
            _ => false,
        }
    }

    /// The items and grenades it can use now. A companion has only the
    /// grenades the party may spend (`choose::may_spend`). Any other
    /// character has every item it carries that a button uses. A grenade
    /// needs a target within the farthest throw. Another item waits for an
    /// item use under way.
    fn items(&self) -> Vec<(Option_, Cast)> {
        let pack = &self.fighter.pack;
        let busy = pack.using.is_some() || pack.queued.is_some();
        self.sheet
            .items
            .iter()
            .enumerate()
            .filter_map(|(at, carried)| {
                let carried = (*carried)?;
                let left = carried.count.saturating_sub(pack.used[at]);
                let used = items::use_of(carried.row)?;
                let grenade = used == items::Use::Grenade;
                let spendable = choose::may_spend(u32::from(left));
                let allowed = if self.companion {
                    grenade && spendable
                } else {
                    left > 0
                };
                if !allowed {
                    return None;
                }
                let cast = if grenade {
                    let distance = self.target.filter(|far| *far <= THROW_FARTHEST)?;
                    ai::throw(distance)
                } else if busy {
                    return None;
                } else {
                    Cast::Hold(used.button(), 1)
                };
                let (kind, droids_only) =
                    choose::kind_of(carried.row, (self.hostile)(carried.row))?;
                let option = Option_ {
                    choice: if grenade {
                        Choice::Grenade(carried.row)
                    } else {
                        Choice::Item(carried.row)
                    },
                    kind,
                    droids_only,
                    running: match used {
                        items::Use::Stim(_) => pack.stim.is_some(),
                        items::Use::Shield { .. } => pack.shield.is_some(),
                        _ => false,
                    },
                };
                Some((option, cast))
            })
            .collect()
    }
}

/// The hit points `fighter` has left as a part of `sheet`'s maximum, from
/// 0 to 1. Hit points are kept in hundredths.
pub(super) fn health(fighter: &Fighter, sheet: &Sheet) -> f32 {
    fighter.hp as f32 / (f32::from(sheet.max_hp) * 100.0).max(1.0)
}

/// How far apart two places are (units).
fn apart(a: Vec3, b: Vec3) -> f32 {
    math::length(math::sub(a, b))
}

/// What a mind with id `me` sees of its target `entry`, `distance` units
/// away: whether the target is within the reach of its weapon (`in_reach`),
/// and the hostiles and the mind's allies within a grenade's blast of the
/// target. The mind itself counts among its allies there.
pub(super) fn target(
    entry: &Seen,
    (distance, in_reach): (f32, bool),
    seen: &[Seen],
    me: SimId,
    hostile: &dyn Fn(&Seen) -> bool,
    ally: &dyn Fn(&Seen) -> bool,
) -> Target {
    let near = |other: &&Seen| apart(other.ps.origin, entry.ps.origin) <= grenades::RADIUS;
    let count = |of: &dyn Fn(&Seen) -> bool| {
        let counted = seen.iter().filter(near).filter(|other| of(other)).count();
        u8::try_from(counted).unwrap_or(u8::MAX)
    };
    Target {
        distance,
        in_reach,
        disabled: entry.stunned || entry.slowed || entry.floored,
        staggered: entry.reeling,
        droid: entry.droid,
        hostiles_near: count(&|other| other.id != entry.id && hostile(other)),
        allies_near: count(&|other| other.id == me || ally(other)),
    }
}

/// The lowest health, as a part of the maximum, among the mind and those of
/// its allies its best healing power reaches from `at`. A heal is cast on
/// the caster's side: it heals every hurt ally within its reach, the
/// caster too (`world::powers`). Without a healing power it is the mind's
/// own health.
pub(super) fn lowest_ally_health(
    sheet: &Sheet,
    (at, own): (Vec3, f32),
    seen: &[Seen],
    ally: &dyn Fn(&Seen) -> bool,
) -> f32 {
    let Some((_, level)) = force::best(&sheet.powers, Power::Heal) else {
        return own;
    };
    let reach = force::ja::HEAL_RANGE[usize::from(level.clamp(1, 3) - 1)];
    seen.iter()
        .filter(|entry| ally(entry) && apart(entry.ps.origin, at) <= reach)
        .map(|entry| entry.health)
        .fold(own, f32::min)
}

/// `entry` as the test for an intense fight sees it.
pub(super) fn member(entry: &Seen) -> Member {
    Member {
        level: entry.sheet.level,
        health: entry.health,
        boss: entry.boss,
    }
}

/// Writes `choice` to the standard error when the choice log is on: the
/// time, the chooser's tag, the choice, its score and why it scored.
pub(super) fn log(tag: &str, option: &Option_, scene: &Scene) {
    if !choose::logging() {
        return;
    }
    let reason = match option.kind {
        Kind::Heal => format!(
            "the most hurt ally has {:.0}% of its hit points",
            scene.lowest_ally_health * 100.0
        ),
        Kind::Buff => "the fight has just begun".to_string(),
        Kind::Disable => "the target is not disabled".to_string(),
        Kind::Damage => "the target is out of reach or staggered".to_string(),
        Kind::AreaDamage => format!(
            "{} enemies stand in the blast",
            scene.target.map_or(0, |target| target.hostiles_near + 1)
        ),
    };
    eprintln!(
        "choice {} ms {tag} {:?} {:?} score {:.2}: {reason}",
        scene.now,
        option.kind,
        option.choice,
        choose::score(option, scene)
    );
}
