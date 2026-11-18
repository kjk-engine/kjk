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
    /// The character itself is a droid.
    pub own_droid: bool,
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
        Kind::Heal | Kind::Buff if !fits_own_body(option, scene) => 0.0,
        Kind::Heal => {
            // An item heals its user, and a power heals the most hurt ally.
            let health = if item {
                scene.own_health
            } else {
                scene.lowest_ally_health
            };
            heal_score(health)
        }
        Kind::Buff => yes(scene.fight_ms < choices::OPENING_MS && !option.running),
        Kind::Disable | Kind::Damage | Kind::AreaDamage => {
            let Some(target) = &scene.target else {
                return 0.0;
            };
            let blast =
                matches!(option.choice, Choice::Grenade(_)) || option.kind == Kind::AreaDamage;
            if blast && !worth_a_blast(option, scene, target) {
                return 0.0;
            }
            // A choice that works only on droids is wasted on a living target.
            if option.droids_only && !target.droid {
                return 0.0;
            }
            match option.kind {
                // A disable that is not made for droids does not work on a droid.
                Kind::Disable => yes(!target.disabled && (option.droids_only || !target.droid)),
                Kind::Damage => yes(!target.in_reach || target.staggered),
                Kind::AreaDamage => 1.0,
                Kind::Heal | Kind::Buff => 0.0,
            }
        }
    }
}

/// Whether a heal or a buff, used on the character itself, works on its
/// body. A choice that works only on droids, such as a repair kit, works
/// only for a droid. A medpac does nothing for a droid.
fn fits_own_body(option: &Option_, scene: &Scene) -> bool {
    let heal_item = option.kind == Kind::Heal && matches!(option.choice, Choice::Item(_));
    if option.droids_only {
        scene.own_droid
    } else {
        !(heal_item && scene.own_droid)
    }
}

/// A heal's score for a character at `health`. It is 0 at or above the
/// heal's cut-off. Just below the cut-off it is just above the threshold,
/// and it rises in a straight line to 1 at a health of 0.
fn heal_score(health: f32) -> f32 {
    if health >= choices::HEAL_BELOW {
        return 0.0;
    }
    let hurt = ((choices::HEAL_BELOW - health) / choices::HEAL_BELOW).clamp(0.0, 1.0);
    choices::THRESHOLD + (1.0 - choices::THRESHOLD) * hurt
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

/// The best option whose score passes the threshold. A heal that passes is
/// chosen ahead of every option of another kind. Options whose scores are
/// within the tie of the best count as equally good, and `pick` (0 to 1)
/// selects one of them in the order given.
pub fn choose(options: &[Option_], scene: &Scene, pick: f32) -> Option<Choice> {
    let passing: Vec<(&Option_, f32)> = options
        .iter()
        .map(|option| (option, score(option, scene)))
        .filter(|&(_, score)| score >= choices::THRESHOLD)
        .collect();
    let heals = passing.iter().any(|(option, _)| option.kind == Kind::Heal);
    let group: Vec<(Choice, f32)> = passing
        .iter()
        .filter(|(option, _)| !heals || option.kind == Kind::Heal)
        .map(|&(option, score)| (option.choice, score))
        .collect();
    let best = group.iter().map(|&(_, score)| score).fold(0.0, f32::max);
    let tied: Vec<Choice> = group
        .iter()
        .filter(|&&(_, score)| best - score <= choices::TIE)
        .map(|&(choice, _)| choice)
        .collect();
    let at = (pick.clamp(0.0, 1.0) * tied.len() as f32) as usize;
    tied.get(at.min(tied.len().saturating_sub(1))).copied()
}

/// Every option whose score passes the threshold, in the order the
/// character prefers them. The first is what `choose` picks. A passing heal
/// comes before every option of another kind. After that the options go
/// from the highest score to the lowest, and options with the same score
/// keep the order given. A character tries the next one when it cannot use
/// the one before this look.
pub fn ranked(options: &[Option_], scene: &Scene, pick: f32) -> Vec<Choice> {
    let Some(first) = choose(options, scene, pick) else {
        return Vec::new();
    };
    let mut rest: Vec<(&Option_, f32)> = options
        .iter()
        .map(|option| (option, score(option, scene)))
        .filter(|&(option, score)| score >= choices::THRESHOLD && option.choice != first)
        .collect();
    rest.sort_by(|(a, a_score), (b, b_score)| {
        let heal = |option: &Option_| option.kind != Kind::Heal;
        heal(a).cmp(&heal(b)).then(b_score.total_cmp(a_score))
    });
    std::iter::once(first)
        .chain(rest.into_iter().map(|(option, _)| option.choice))
        .collect()
}

/// What a character sees of its target that decides whether some powers
/// would do anything to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mark {
    /// A Force Scream cannot make the target cover its ears now.
    pub deaf: bool,
    /// The lowest level of the powers running on the target that a dispel
    /// could end, if any run.
    pub buff: Option<u8>,
}

/// Whether power `row` would do anything to a target with `mark`. Force
/// Scream does nothing to a target that cannot be made to cover its ears.
/// Force Suppression ends powers of the first two levels, and Force Breach
/// ends powers of every level, so each does nothing to a target with no
/// power it can end. Every other row may do something.
pub fn does_something(row: u16, mark: Mark) -> bool {
    use crate::combat::force;
    use crate::combat::spells::{FORCE_POWER_FORCE_BREACH, FORCE_POWER_SUPRESS_FORCE};
    if force::crowd::scream(row) {
        return !mark.deaf;
    }
    match row {
        FORCE_POWER_SUPRESS_FORCE => mark.buff.is_some_and(|buff| buff <= SUPRESSED_LEVELS),
        FORCE_POWER_FORCE_BREACH => mark.buff.is_some(),
        _ => true,
    }
}

/// Whether a character at `own_health` (a part of its maximum hit points,
/// from 0 to 1) is wise to use power `row` on itself. A character uses
/// Rage only while it has at least `ai.choices.rage_above` of its hit
/// points left. Every other row is wise.
pub fn wise_for(row: u16, own_health: f32) -> bool {
    use crate::combat::force::{self, Power};
    !matches!(force::of_row(row), Some((Power::Rage, _))) || own_health >= choices::RAGE_ABOVE
}

/// The highest level of power Force Suppression ends. Force Breach, the
/// next level of the line, ends powers of every level.
const SUPRESSED_LEVELS: u8 = choices::SUPRESSED_LEVELS;

/// A living character on one side of a fight, as the test for an intense
/// fight sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Member {
    pub level: u8,
    /// Its hit points as a part of its maximum, from 0 to 1.
    pub health: f32,
    pub boss: bool,
}

/// Whether the fight is intense for the party right now. It is intense when
/// the hostiles are at least as strong as the party, when enough hostiles
/// are alive, or when a member of the party is badly hurt. A fight with no
/// living hostiles is never intense.
pub fn intense_now(party: &[Member], hostiles: &[Member]) -> bool {
    if hostiles.is_empty() {
        return false;
    }
    let outnumbered = hostiles.len() >= choices::INTENSE_HOSTILES as usize;
    let stronger = strength(hostiles) >= choices::INTENSE_STRENGTH_SHARE * strength(party);
    let hurt = party
        .iter()
        .any(|member| member.health < choices::INTENSE_HURT_BELOW);
    outnumbered || stronger || hurt
}

/// The strength of one side: each character's level multiplied by the part
/// of its hit points it has left, added up. A Boss counts more.
fn strength(side: &[Member]) -> f32 {
    side.iter()
        .map(|member| {
            let plain = f32::from(member.level) * member.health;
            if member.boss {
                plain * choices::INTENSE_BOSS_FACTOR
            } else {
                plain
            }
        })
        .sum()
}

/// The time until which a fight stays intense after a look at `now` found
/// it intense.
pub fn held_until(now: u64) -> u64 {
    now + choices::INTENSE_HOLD_MS
}

/// Whether the fight is intense at `now` for a mind whose last intense look
/// set `intense_until`.
pub fn intense(now: u64, intense_until: u64) -> bool {
    now < intense_until
}

/// Whether a companion may use an item of a kind the party has `left` of.
/// A companion never uses the last few items of each kind.
pub fn may_spend(left: u32) -> bool {
    left > choices::KEEP_ITEMS
}

/// Whether each choice is written to the standard error (`log_choices`).
static LOG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Turns the choice log on or off. The launcher turns it on once at its
/// start when the environment has `KJK_CHOICE_LOG=1`. The log only writes
/// lines. It never changes a choice.
pub fn log_choices(on: bool) {
    LOG.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the choice log is on.
pub fn logging() -> bool {
    LOG.load(std::sync::atomic::Ordering::Relaxed)
}

/// The wait after a choice for a character of `tier`. `pick` (0 to 1)
/// places it between the tier's least and most wait.
pub fn wait_ms(tier: Tier, pick: f32) -> u64 {
    let [least, most] = match tier {
        Tier::Ordinary => wait_ms::ORDINARY,
        Tier::Skilled => wait_ms::SKILLED,
        Tier::Boss => wait_ms::BOSS,
    };
    least + (most.saturating_sub(least) as f32 * pick.clamp(0.0, 1.0)) as u64
}
