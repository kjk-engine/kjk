//! How the combat AI chooses a Force power, an item or a grenade of its own
//! (`ai.choices`). Each option a character can use now is scored from 0 to
//! 1 against what the character sees. The best option that passes the
//! threshold is used. This module is pure: the caller gathers the facts and
//! passes `pick` in, so the same inputs always give the same choice.

use rules::ai::choices;
use rules::ai::choices::wait_ms;

use crate::combat::pace::Tier;
pub use crate::combat::spells::Kind;
use crate::combat::spells::kind_of_row;

/// One thing a character can use now, by its `spells.2da` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Power(u16),
    Item(u16),
    Grenade(u16),
}

/// A choice with the facts the scoring reads about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Option_ {
    pub choice: Choice,
    pub kind: Kind,
    /// The choice works only on droids.
    pub droids_only: bool,
    /// The choice's effect is already running on the character it would
    /// strengthen.
    pub running: bool,
}

/// What the character sees of its target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub distance: f32,
    /// The target stands inside the reach of the character's weapon.
    pub in_reach: bool,
    /// The target is stunned, knocked down or slowed.
    pub disabled: bool,
    pub staggered: bool,
    pub droid: bool,
    /// Other hostiles within the blast radius of the target.
    pub hostiles_near: u8,
    /// The character's allies within the blast radius of the target.
    pub allies_near: u8,
}

/// What the character sees of the fight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scene {
    pub now: u64,
    /// The time since the character became aware of a foe.
    pub fight_ms: u64,
    pub intense: bool,
    pub companion: bool,
    /// The character's hit points as a part of its maximum, from 0 to 1.
    pub own_health: f32,
    /// The lowest hit points among the character and its allies, as a part
    /// of each one's maximum, from 0 to 1.
    pub lowest_ally_health: f32,
    pub target: Option<Target>,
}

/// The kind of `row`, and whether the row works only on droids. The
/// rulebook's table decides when it lists the row. Otherwise a hostile row
/// is a plain damage power and a friendly row is a plain buff. `hostile` is
/// `None` when there are no talent facts for the row, and then the row has
/// no kind.
pub fn kind_of(row: u16, hostile: Option<bool>) -> Option<(Kind, bool)> {
    kind_of_row(row).or_else(|| {
        hostile.map(|hostile| {
            let kind = if hostile { Kind::Damage } else { Kind::Buff };
            (kind, false)
        })
    })
}

/// How good `option` is now, from 0 to 1. A score of 0 means the character
/// has no reason to use it.
pub fn score(option: &Option_, scene: &Scene) -> f32 {
    let yes = |reason: bool| if reason { 1.0 } else { 0.0 };
    let item = matches!(option.choice, Choice::Item(_));
    // A companion's medpacs, stims and shields are left to KOTOR's own scripts.
    if item && scene.companion {
        return 0.0;
    }
    match option.kind {
        Kind::Heal => {
            // An item heals its user, and a power heals the most hurt ally.
            let health = if item {
                scene.own_health
            } else {
                scene.lowest_ally_health
            };
            ((choices::HEAL_BELOW - health) / choices::HEAL_BELOW).clamp(0.0, 1.0)
        }
        Kind::Buff => yes(scene.fight_ms < choices::OPENING_MS && !option.running),
        kind => {
            let Some(target) = &scene.target else {
                return 0.0;
            };
            let blast = matches!(option.choice, Choice::Grenade(_)) || kind == Kind::AreaDamage;
            if blast && !worth_a_blast(option, scene, target) {
                return 0.0;
            }
            match kind {
                Kind::Disable => yes(!target.disabled && option.droids_only == target.droid),
                Kind::Damage => yes(!target.in_reach || target.staggered),
                _ => 1.0,
            }
        }
    }
}

/// Whether a blast around `target` hits enough enemies and none of the
/// character's allies. A companion throws a grenade only in an intense fight.
fn worth_a_blast(option: &Option_, scene: &Scene, target: &Target) -> bool {
    let enemies = u32::from(target.hostiles_near) + 1;
    let grenade = matches!(option.choice, Choice::Grenade(_));
    enemies >= choices::CLUSTER_TARGETS
        && target.allies_near == 0
        && !(grenade && scene.companion && !scene.intense)
}

/// The best option whose score passes the threshold. Options whose scores
/// are within the tie of the best count as equally good, and `pick` (0 to
/// 1) selects one of them in the order given.
pub fn choose(options: &[Option_], scene: &Scene, pick: f32) -> Option<Choice> {
    let scored: Vec<(Choice, f32)> = options
        .iter()
        .map(|option| (option.choice, score(option, scene)))
        .filter(|&(_, score)| score >= choices::THRESHOLD)
        .collect();
    let best = scored.iter().map(|&(_, score)| score).fold(0.0, f32::max);
    let tied: Vec<Choice> = scored
        .iter()
        .filter(|&&(_, score)| best - score <= choices::TIE)
        .map(|&(choice, _)| choice)
        .collect();
    let at = (pick.clamp(0.0, 1.0) * tied.len() as f32) as usize;
    tied.get(at.min(tied.len().saturating_sub(1))).copied()
}

/// The wait after a choice for a character of `tier`. `pick` (0 to 1)
/// places it between the tier's least and most wait.
pub fn wait_ms(tier: Tier, pick: f32) -> u64 {
    let [least, most] = match tier {
        Tier::Ordinary => wait_ms::ORDINARY,
        Tier::Skilled => wait_ms::SKILLED,
        Tier::Boss => wait_ms::BOSS,
    };
    least + ((most - least) as f32 * pick.clamp(0.0, 1.0)) as u64
}
