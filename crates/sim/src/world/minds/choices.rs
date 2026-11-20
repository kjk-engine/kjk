//! What a mind needs to choose a Force power, an item or a grenade of its
//! own (`combat::choose`). This module lists the options the mind has now.
//! It reads what the mind sees of its target and of its allies. It finds
//! the members of each side of the party's fight. It also writes the line
//! of the choice log.

use std::collections::BTreeMap;

use collision::UNIT;

use super::Seen;
use crate::combat::Fighter;
use crate::combat::ai::{self, Cast};
use crate::combat::choose::{self, Choice, Foes, Kind, Member, Option_, Reason, Scene, Target};
use crate::combat::force::{self, Power};
use crate::combat::gear;
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
    /// The distance to its target (units), and what decides whether some
    /// powers would do anything to it, if it has a target.
    pub target: Option<(f32, choose::Mark)>,
    /// How many items of a row the other members of its party have used,
    /// queued or are charging that the party's inventory still counts
    /// (`Pack::not_yet_taken`).
    pub untaken: &'a dyn Fn(u16) -> u8,
    /// What a power costs it (hundredths of a Force point).
    pub cost: &'a dyn Fn(u16) -> Option<i32>,
    pub scripted: ai::Scripted<'a>,
    /// The talent facts' `hostile` for a row, where the scripts' library
    /// has the row.
    pub hostile: &'a dyn Fn(u16) -> Option<bool>,
    /// The powers each power needs (`spells.2da` `prerequisites`), where a
    /// script library is loaded.
    pub prerequisites: Option<&'a BTreeMap<u16, [Option<u16>; 2]>>,
    /// The distance to the nearest fallen member of its party (units), if
    /// one has fallen.
    pub fallen: Option<f32>,
}

impl Chooser<'_> {
    /// Every option it has now, each with the cast that uses it.
    pub fn options(&self) -> Vec<(Option_, Cast)> {
        let mut options = self.powers();
        options.extend(self.items());
        options
    }

    /// The Force powers it can use now. Each power must pass these tests.
    /// The power is the highest rank it knows of its line
    /// (`choose::highest_known`). It has the Force points for the power,
    /// and the power's own wait is over. A power used
    /// on an enemy reaches its target and would do something to it
    /// (`choose::does_something`). Revitalize is listed only when a fallen
    /// member of its party is within the power's reach.
    fn powers(&self) -> Vec<(Option_, Cast)> {
        let powers = &self.sheet.powers;
        powers
            .rows()
            .filter(|&row| choose::highest_known(row, powers, self.prerequisites))
            .filter(|&row| force::of_row(row).is_none_or(|(power, _)| self.power_ready(power)))
            .filter(|&row| choose::wise_for(row, health(self.fighter, self.sheet)))
            .filter_map(|row| {
                let (kind, droids_only) = choose::kind_of(row, (self.hostile)(row))?;
                let at_enemy = matches!(kind, Kind::Disable | Kind::Damage | Kind::AreaDamage);
                let reach = ai::power_reach(row, self.scripted);
                let works = |&(distance, mark): &(f32, choose::Mark)| {
                    distance <= reach && choose::does_something(row, mark)
                };
                if at_enemy && !self.target.as_ref().is_some_and(works) {
                    return None;
                }
                let raises = choose::raises_the_fallen(Choice::Power(row));
                if raises && !self.fallen.is_some_and(|distance| distance <= reach) {
                    return None;
                }
                let cast = ai::power_cast(row, self.sheet, self.fighter, self.cost, self.scripted)?;
                let option = Option_ {
                    choice: Choice::Power(row),
                    kind,
                    droids_only,
                    running: self.row_running(row),
                };
                Some((option, cast))
            })
            .collect()
    }

    /// Whether the effect of power `row` already runs on it. The engine's
    /// own effect of the power's line counts (`running`). So does a lasting
    /// effect that the impact script of `row`, or of a higher rank of its
    /// line, applied to it (`choose::covers`). Such an effect may come from
    /// another character, as a party member's Battle Meditation does. This
    /// is the test of the script routine `GetHasSpellEffect`.
    ///
    /// Like that routine, it counts any running effect from the row, of
    /// whatever kind and from whichever caster.
    fn row_running(&self, row: u16) -> bool {
        let own = force::of_row(row).is_some_and(|(power, _)| self.running(power));
        own || (self.fighter.effects.running(self.now))
            .any(|effect| choose::covers(effect.spell, row, self.prerequisites))
    }

    /// Whether a buff whose reason is an intense fight runs on it
    /// (`choose::Reason::Intense`). The engine's own Valor counts, and so
    /// does a running effect from any row of that reason.
    pub fn intense_buff(&self) -> bool {
        let intense = |row: u16| choose::reason_of(Choice::Power(row)) == Reason::Intense;
        self.running(Power::Valor)
            || (self.fighter.effects.running(self.now)).any(|effect| intense(effect.spell))
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
    /// grenades the party may spend (`choose::may_spend`). The grenades
    /// that the other members of the party have thrown or are about to
    /// throw already count as spent, even before the party's inventory
    /// loses them. Any other character has every item it carries that a
    /// button uses. A rocket or a dart is listed only while it wears a
    /// wrist launcher (`grenades::usable`). A grenade needs a target within
    /// the farthest throw. Another item waits for an item use under way.
    /// Of its healing items, only the one that best fits the hit points it
    /// is missing is listed (`fitting_heal`).
    fn items(&self) -> Vec<(Option_, Cast)> {
        let pack = &self.fighter.pack;
        let busy = pack.using.is_some() || pack.queued.is_some();
        let launcher = self.sheet.gear.has(gear::traits::WRIST_LAUNCHER);
        let mut listed = grenades::usable(&self.sheet.items, launcher)
            .iter()
            .filter_map(|carried| {
                let carried = (*carried)?;
                // Its own uses and its own coming throw of the row are not
                // left, though the party's inventory still counts them.
                let own = pack.not_yet_taken(self.sheet, carried.row, self.now);
                let left = carried.count.saturating_sub(own);
                let used = items::use_of(carried.row)?;
                let grenade = used == items::Use::Grenade;
                let unthrown = left.saturating_sub((self.untaken)(carried.row));
                let spendable = choose::may_spend(u32::from(unthrown));
                let allowed = if self.companion {
                    grenade && spendable
                } else {
                    left > 0
                };
                if !allowed {
                    return None;
                }
                let cast = if grenade {
                    let (distance, _) = self.target.filter(|(far, _)| *far <= THROW_FARTHEST)?;
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
            .collect::<Vec<_>>();
        // Of the healing items that work on its body, it keeps only the one
        // it would use (`fitting_heal`).
        let fitting = self.fitting_heal(listed.iter().map(|(option, _)| option.choice));
        listed.retain(|(option, _)| match option.choice {
            Choice::Item(row) => self.heals_body(row).is_none() || fitting == Some(row),
            _ => true,
        });
        listed
    }

    /// What a healing item of `row` restores it (whole points), when the
    /// item heals its kind of body: a medpac a living character, a repair
    /// kit a droid. `None` for any other item.
    fn heals_body(&self, row: u16) -> Option<i32> {
        let droid = self.sheet.flags & crate::combat::sheet::flags::DROID != 0;
        match items::use_of(row)? {
            items::Use::Heal {
                points,
                per_rank,
                droid: kit,
            } if kit == droid => Some(items::healing(self.sheet, points, per_rank, kit)),
            _ => None,
        }
    }

    /// The healing item it would use of the `choices` it has. It is the
    /// weakest one that restores all the hit points it is missing. When
    /// none restores them all, it is the strongest. A medpac that ties
    /// keeps the order given.
    fn fitting_heal(&self, choices: impl Iterator<Item = Choice>) -> Option<u16> {
        let missing = i32::from(self.sheet.max_hp) - self.fighter.hp / 100;
        let heals: Vec<(u16, i32)> = choices
            .filter_map(|choice| match choice {
                Choice::Item(row) => Some((row, self.heals_body(row)?)),
                _ => None,
            })
            .collect();
        let enough = heals.iter().filter(|(_, restores)| *restores >= missing);
        enough
            .min_by_key(|(_, restores)| *restores)
            .or_else(|| heals.iter().max_by_key(|(_, restores)| *restores))
            .map(|(row, _)| *row)
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

/// What a mind with id `me` sees of its target `entry`. `in_reach` says
/// whether the target is within the reach of the mind's weapon. The mind
/// counts the hostiles and its own allies within a grenade's blast of the
/// target. The mind itself counts among its allies there.
pub(super) fn target(
    entry: &Seen,
    in_reach: bool,
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
/// the caster's side. It heals every hurt ally within its reach, and the
/// caster too (`world::powers`). Without a healing power it is the mind's
/// own health. The healing powers are Cure, Heal and Master Heal
/// (`Power::Heal`). Revitalize is not one of them, because it only raises
/// the fallen.
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

/// What the hostiles in its fight fight with (`choose::Foes`). `in_fight`
/// says whether a creature it sees is a living hostile in its fight.
pub(super) fn foes(seen: &[Seen], in_fight: &dyn Fn(&Seen) -> bool) -> Foes {
    use crate::combat::effects::RESIST_TYPES;
    use crate::combat::sheet::damage_types::{BLUDGEONING, PIERCING, SLASHING};
    use crate::combat::sheet::type_rows;
    // The damage types Energy Resistance resists: the engine's own effect
    // (`effects::RESIST_TYPES`), and energy, which both games' impact
    // script for the power resists too.
    let resisted = (RESIST_TYPES.iter().chain([&type_rows::ENERGY]))
        .fold(0u16, |types, row| types | type_bit(*row));
    let knows = |entry: &Seen, power: &dyn Fn(u16) -> bool| entry.sheet.powers.rows().any(power);
    let lightning = |row: u16| matches!(force::of_row(row), Some((Power::Lightning, _)));
    let at_foes = |row: u16| {
        let kind = choose::kind_of(row, None).map(|(kind, _)| kind);
        matches!(kind, Some(Kind::Disable | Kind::Damage | Kind::AreaDamage))
    };
    (seen.iter().filter(|entry| in_fight(entry))).fold(Foes::default(), |foes, entry| Foes {
        energy: foes.energy || entry.damage & resisted != 0 || knows(entry, &lightning),
        physical: foes.physical
            || entry.beast
            || entry.damage & (BLUDGEONING | PIERCING | SLASHING) != 0,
        force: foes.force || knows(entry, &at_foes),
    })
}

/// The `damageflags` bit of the damage type row `row`
/// (`sheet::DAMAGE_TYPES`). A row past the last type has no bit.
pub(super) fn type_bit(row: u8) -> u16 {
    1u16.checked_shl(u32::from(row)).unwrap_or(0)
}

/// `entry` as the test for an intense fight sees it.
pub(super) fn member(entry: &Seen) -> Member {
    Member {
        level: entry.sheet.level,
        health: entry.health,
        boss: entry.boss,
    }
}

/// Writes `choice` to the standard error when the choice log is on. The
/// line has the time, the chooser's tag, the choice, its score and the
/// reason for the score.
pub(super) fn log(tag: &str, option: &Option_, scene: &Scene) {
    if !choose::logging() {
        return;
    }
    let reason = match option.kind {
        Kind::Heal => format!(
            "the most hurt ally has {:.0}% of its hit points",
            scene.lowest_ally_health * 100.0
        ),
        Kind::Buff => match choose::reason_of(option.choice) {
            Reason::Energy => "an enemy holds an energy weapon or knows lightning",
            Reason::Physical => "an enemy is a beast or fights with a physical weapon",
            Reason::ForceUsers => "an enemy knows Force powers it uses on its foes",
            Reason::Intense => "the fight is intense",
            Reason::Blade => "the character fights in melee",
            Reason::Always => "the fight has just begun",
        }
        .to_string(),
        Kind::Disable => "the target is not disabled".to_string(),
        Kind::Damage if scene.target.is_some_and(|target| !target.in_reach) => {
            "the target is out of reach".to_string()
        }
        Kind::Damage => "the target is staggered".to_string(),
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
