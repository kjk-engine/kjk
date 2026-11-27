//! How the combat AI chooses a Force power, an item or a grenade of its own
//! (`ai.choices`). Each option a character can use now is scored from 0 to
//! 1 against what the character sees. The best option that passes the
//! threshold is used. The AI also chooses its feat line, the saber style of
//! a single blade, by the reasons in `feat_line` (`ai.choices.feats`). This
//! module is pure. The caller gathers the facts and passes `pick` in. The
//! same inputs always lead to the same choice.

use std::collections::{BTreeMap, BTreeSet};

use rules::ai::choices;
use rules::ai::choices::feats;
use rules::ai::choices::reasons;
use rules::ai::choices::scores;
use rules::ai::choices::wait_ms;

use crate::combat::bar::FeatLine;
use crate::combat::pace::Tier;
use crate::combat::saber::Style;
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
    /// The character itself fights in melee. It holds a lightsaber, a
    /// sword, a vibroblade or a stun baton, or it has empty hands.
    pub own_blade: bool,
    /// What the hostiles in the fight fight with.
    pub foes: Foes,
    /// A buff whose reason is an intense fight runs on the character. It
    /// may be the character's own or another character's.
    pub intense_buff: bool,
    pub target: Option<Target>,
}

/// What the living hostiles in the character's fight fight with. The
/// reasons of the buffs read these facts (`fits`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Foes {
    /// A hostile holds a weapon that deals a damage type Energy Resistance
    /// resists, such as a blaster or a lightsaber, or it knows a lightning
    /// power. Damage that a property of the weapon adds counts too.
    pub energy: bool,
    /// A hostile is a beast, or it holds a weapon that deals bludgeoning,
    /// piercing or slashing damage. A vibroblade and empty hands count.
    pub physical: bool,
    /// A hostile knows a Force power that it uses on its foes.
    pub force: bool,
}

/// Why a buff may be worth casting (`ai.choices.reasons`). A buff is used
/// only while its reason is true (`fits`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// A hostile deals energy damage (`Foes::energy`).
    Energy,
    /// A hostile deals physical damage (`Foes::physical`).
    Physical,
    /// A hostile uses Force powers on its foes (`Foes::force`).
    ForceUsers,
    /// The fight is intense for the party.
    Intense,
    /// The character itself fights in melee.
    Blade,
    /// Every other buff, a mod's too.
    Always,
}

/// The rulebook's rows of each reason (`ai.choices.reasons`). A buff in no
/// list has the reason `Always`.
pub const ROWS_BY_REASON: [(Reason, &[u16]); 5] = [
    (Reason::Energy, &reasons::ENERGY),
    (Reason::Physical, &reasons::PHYSICAL),
    (Reason::ForceUsers, &reasons::FORCE_USERS),
    (Reason::Intense, &reasons::INTENSE),
    (Reason::Blade, &reasons::BLADE),
];

/// The reason of the buff `choice`. An item, a grenade and a power that no
/// list names have the reason `Always`.
pub fn reason_of(choice: Choice) -> Reason {
    let Choice::Power(row) = choice else {
        return Reason::Always;
    };
    ROWS_BY_REASON
        .iter()
        .find(|(_, rows)| rows.contains(&row))
        .map_or(Reason::Always, |(reason, _)| *reason)
}

/// Whether `reason` is true in `scene`.
pub fn fits(reason: Reason, scene: &Scene) -> bool {
    match reason {
        Reason::Energy => scene.foes.energy,
        Reason::Physical => scene.foes.physical,
        Reason::ForceUsers => scene.foes.force,
        Reason::Intense => scene.intense,
        Reason::Blade => scene.own_blade,
        Reason::Always => true,
    }
}

/// Whether a buff with `reason` may be used at `scene`'s moment of the
/// fight. Plain buffs (`Reason::Always`: Force Aura, stims, energy shields,
/// and unlisted friendly powers) are used in the first
/// `ai.choices.opening_ms` (15000 ms) of a fight. Tactical buffs that answer a
/// specific threat (`Energy`, `Physical`, `ForceUsers`, `Blade`) may be used
/// at any time while their reason holds. Intense-fight buffs (`Reason::Intense`)
/// are used only while no buff of that reason runs on the character, so the
/// character keeps one up at a time and does not spend Force points stacking
/// a second one.
fn in_time(reason: Reason, scene: &Scene) -> bool {
    match reason {
        Reason::Energy | Reason::Physical | Reason::ForceUsers | Reason::Blade => true,
        Reason::Intense => !scene.intense_buff,
        Reason::Always => scene.fight_ms < choices::OPENING_MS,
    }
}

/// What `spells.2da` says about a row (`Talent::hostile` and
/// `Talent::category`), used to classify unlisted mod powers (`kind_of`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellFacts {
    pub hostile: bool,
    pub category: u16,
}

/// `spells.2da` `category` bits used to classify unlisted mod powers.
const CAT_AREA: u16 = 0x2000;
const CAT_DAMAGE: u16 = 0x0100;
const CAT_DISABLE: u16 = 0x0200;
const CAT_HEAL: u16 = 0x0400;
const CAT_DEBUFF: u16 = 0x0010;

/// The kind of `row`, and whether the row works only on droids. The
/// rulebook's table decides when it lists the row. A row of the games' own
/// that the engine names (`spells::NAMED`) and the table does not list has
/// no kind, so it is never a choice. Affect Mind, Dominate Mind and Force
/// Sight are such rows. So is a row of the games' own that the engine does
/// not name and the rulebook lists as never chosen (`ai.choices.kinds.never`),
/// such as Force Camouflage and Breath Control. Any other row comes from a
/// mod and is classified from `facts` (`spells.2da` `hostilesetting` and
/// `category`): friendly heals become `Heal`, other friendly powers become
/// `Buff`, hostile area-damage rows become `AreaDamage`, pure hostile
/// disables/debuffs become `Disable`, and all other hostile rows become
/// `Damage`. `facts` is `None` when there are no talent facts for the row,
/// and then the row has no kind.
pub fn kind_of(row: u16, facts: Option<SpellFacts>) -> Option<(Kind, bool)> {
    use crate::combat::spells::NAMED;
    kind_of_row(row).or_else(|| {
        let named = NAMED.iter().any(|&(_, named)| named == row);
        if named || choices::kinds::NEVER.contains(&row) {
            return None;
        }
        facts.map(|SpellFacts { hostile, category }| {
            let kind = if !hostile {
                if category & CAT_HEAL != 0 {
                    Kind::Heal
                } else {
                    Kind::Buff
                }
            } else if category & CAT_AREA != 0 && category & CAT_DAMAGE != 0 {
                Kind::AreaDamage
            } else if (category & CAT_DISABLE != 0 || category & CAT_DEBUFF != 0)
                && category & CAT_DAMAGE == 0
            {
                Kind::Disable
            } else {
                Kind::Damage
            };
            (kind, false)
        })
    })
}

/// Why a choice scored above zero (`score_with_why`), as the choice log
/// writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceWhy {
    FallenAlly,
    OwnHealth,
    AllyHealth,
    Buff(Reason),
    Disable,
    Cluster,
    Staggered,
    OutOfReach,
}

/// How good `option` is now, from 0 to 1, and why. A score of 0 means the
/// character has no reason to use it.
pub fn score(option: &Option_, scene: &Scene) -> f32 {
    score_with_why(option, scene).map_or(0.0, |(score, _)| score)
}

/// How good `option` is now (above 0), and why. `None` when the character
/// has no reason to use it.
pub fn score_with_why(option: &Option_, scene: &Scene) -> Option<(f32, ChoiceWhy)> {
    let item = matches!(option.choice, Choice::Item(_));
    // A companion uses no medpac, stim or shield of its own choice. It uses
    // a medpac only when its own combat script picks one.
    if item && scene.companion {
        return None;
    }
    match option.kind {
        Kind::Heal | Kind::Buff if !fits_own_body(option, scene) => None,
        // Revitalize is offered only when a party member has fallen. A
        // fallen character has no hit points left.
        Kind::Heal if raises_the_fallen(option.choice) => {
            let score = heal_score(0.0);
            (score > 0.0).then_some((score, ChoiceWhy::FallenAlly))
        }
        Kind::Heal => {
            // An item heals its user, and a power heals the most hurt ally.
            let (health, why) = if item {
                (scene.own_health, ChoiceWhy::OwnHealth)
            } else {
                (scene.lowest_ally_health, ChoiceWhy::AllyHealth)
            };
            let score = heal_score(health);
            (score > 0.0).then_some((score, why))
        }
        Kind::Buff => {
            let reason = reason_of(option.choice);
            if in_time(reason, scene) && fits(reason, scene) && !option.running {
                let score = if reason == Reason::Always {
                    scores::PLAIN_BUFF
                } else {
                    scores::COUNTER_BUFF
                };
                Some((score, ChoiceWhy::Buff(reason)))
            } else {
                None
            }
        }
        Kind::Disable | Kind::Damage | Kind::AreaDamage => {
            let target = scene.target.as_ref()?;
            // A choice that works only on droids is wasted on a living target.
            if option.droids_only && !target.droid {
                return None;
            }
            let enemies = u32::from(target.hostiles_near) + 1;
            let cluster_score = if enemies > choices::CLUSTER_TARGETS {
                scores::CLUSTER_LARGE
            } else {
                scores::CLUSTER
            };
            let single_damage = || {
                if target.staggered {
                    Some((scores::STAGGER_DAMAGE, ChoiceWhy::Staggered))
                } else if !target.in_reach {
                    Some((scores::FAR_DAMAGE, ChoiceWhy::OutOfReach))
                } else {
                    None
                }
            };
            // A disable that is not made for droids does not work on a droid.
            let disable_allowed = !target.disabled && (option.droids_only || !target.droid);
            if matches!(option.choice, Choice::Grenade(_)) {
                if !worth_a_grenade(scene, target) {
                    return None;
                }
                return match option.kind {
                    Kind::Disable if !disable_allowed => None,
                    _ => Some((cluster_score, ChoiceWhy::Cluster)),
                };
            }
            match option.kind {
                Kind::Disable => disable_allowed.then_some((scores::DISABLE, ChoiceWhy::Disable)),
                Kind::Damage => single_damage(),
                // Area Force powers (Force Storm, Death Field, Force Scream,
                // Destroy Droid) have no friendly fire, so they do not avoid
                // allies and also hit a single out-of-reach or staggered target.
                Kind::AreaDamage => {
                    if enemies >= choices::CLUSTER_TARGETS {
                        Some((cluster_score, ChoiceWhy::Cluster))
                    } else {
                        single_damage()
                    }
                }
                Kind::Heal | Kind::Buff => None,
            }
        }
    }
}

/// Whether `choice` is a rank of Revitalize, which raises the fallen
/// members of the caster's party (`spells::RAISES_THE_FALLEN`).
pub fn raises_the_fallen(choice: Choice) -> bool {
    use crate::combat::spells::RAISES_THE_FALLEN;
    matches!(choice, Choice::Power(row) if RAISES_THE_FALLEN.contains(&row))
}

/// Whether the character knows a higher rank of the line of power `row`.
/// A rank is higher when it lists `row` as a prerequisite in `spells.2da`
/// (`prerequisites`). `knows` says whether the character knows a row.
///
/// When `force::of_row` puts both rows on the same line, `force::best`
/// already orders them, and the higher row does not count here. So the
/// chooser leaves out a lower rank of a line that `force::of_row` does not
/// map, such as Force Barrier, Battle Meditation, Force Body and
/// Revitalize. It also leaves out a rank that a row outside its mapped line
/// needs. Master Energy Resistance is such a row. It needs Resist Energy 1
/// and 2, and `force::of_row` does not map it.
pub fn outranked(
    row: u16,
    knows: impl Fn(u16) -> bool,
    prerequisites: &BTreeMap<u16, [Option<u16>; 2]>,
) -> bool {
    use crate::combat::force;
    let line = |row: u16| force::of_row(row).map(|(power, _)| power);
    let same_line = |higher: u16| line(higher).is_some() && line(higher) == line(row);
    prerequisites
        .iter()
        .any(|(higher, needs)| knows(*higher) && needs.contains(&Some(row)) && !same_line(*higher))
}

/// Whether `row` is the highest rank of its line among the `powers` a
/// character knows. For a row that `force::of_row` maps, its level must be
/// the best of its mapped line (`force::best`). For every row, the
/// character must know no higher rank that needs it (`outranked`), where
/// the prerequisites of `spells.2da` are loaded.
pub fn highest_known(
    row: u16,
    powers: &crate::combat::sheet::Powers,
    prerequisites: Option<&BTreeMap<u16, [Option<u16>; 2]>>,
) -> bool {
    use crate::combat::force;
    let best_of_line = force::of_row(row).is_none_or(|(power, level)| {
        force::best(powers, power).is_some_and(|(_, best)| best == level)
    });
    let knows = |row: u16| powers.has(row);
    best_of_line && !prerequisites.is_some_and(|needs| outranked(row, knows, needs))
}

/// Whether an effect that power `spell` applied shows that power `row`
/// already runs. It does when `spell` is `row`, or a higher rank of the
/// same line. A rank is higher when it needs `row`, directly or through
/// other ranks, as a prerequisite in `spells.2da` (`prerequisites`).
/// Without prerequisites only `row` itself counts.
pub fn covers(
    spell: u16,
    row: u16,
    prerequisites: Option<&BTreeMap<u16, [Option<u16>; 2]>>,
) -> bool {
    let mut next = vec![spell];
    let mut looked = BTreeSet::new();
    while let Some(at) = next.pop() {
        if at == row {
            return true;
        }
        if looked.insert(at) {
            let needs = prerequisites.and_then(|needs| needs.get(&at));
            next.extend(needs.into_iter().flatten().flatten());
        }
    }
    false
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

/// Whether a grenade blast around `target` hits enough enemies and none of
/// the character's allies. A companion throws a grenade only in an intense
/// fight.
fn worth_a_grenade(scene: &Scene, target: &Target) -> bool {
    let enemies = u32::from(target.hostiles_near) + 1;
    enemies >= choices::CLUSTER_TARGETS
        && target.allies_near == 0
        && !(scene.companion && !scene.intense)
}

/// The best option whose score passes the threshold. A heal that passes is
/// chosen ahead of every option of another kind. Options whose scores are
/// within the tie of the best count as equally good, and `pick` (0 to 1)
/// selects one of them in the order given.
pub fn choose(options: &[Option_], scene: &Scene, pick: f32) -> Option<Choice> {
    ranked(options, scene, pick).first().copied()
}

/// Every option whose score passes the threshold, in the order the
/// character prefers them. A character tries the next one when it cannot
/// use the one before this look.
///
/// A passing heal comes before every option of another kind. After that
/// the options go from the highest score to the lowest, and options with
/// the same score keep the order given. The first entry is one of the
/// equally good leaders, moved to the front. The leaders are the options
/// of the leading kind (heals, when one passes) whose scores are within
/// the tie of the best. `pick` (0 to 1) selects one of them in the order
/// given.
pub fn ranked(options: &[Option_], scene: &Scene, pick: f32) -> Vec<Choice> {
    let mut passing: Vec<(usize, &Option_, f32)> = options
        .iter()
        .enumerate()
        .map(|(at, option)| (at, option, score(option, scene)))
        .filter(|&(_, _, score)| score >= choices::THRESHOLD)
        .collect();
    let not_heal = |option: &Option_| option.kind != Kind::Heal;
    // A stable sort, so that equal options keep the order given.
    passing.sort_by(|(_, a, a_score), (_, b, b_score)| {
        not_heal(a)
            .cmp(&not_heal(b))
            .then(b_score.total_cmp(a_score))
    });
    let Some(&(_, lead, best)) = passing.first() else {
        return Vec::new();
    };
    let mut tied: Vec<usize> = (0..passing.len())
        .filter(|&place| {
            let (_, option, score) = passing[place];
            not_heal(option) == not_heal(lead) && best - score <= choices::TIE
        })
        .collect();
    tied.sort_by_key(|&place| passing[place].0);
    let at = (pick.clamp(0.0, 1.0) * tied.len() as f32) as usize;
    let first = tied[at.min(tied.len() - 1)];
    let chosen = passing.remove(first);
    std::iter::once(chosen)
        .chain(passing)
        .map(|(_, option, _)| option.choice)
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
    /// The target's body is a creature model's, such as a kath hound's or
    /// a rancor's.
    pub beast: bool,
}

/// Whether power `row` would do anything to a target with `mark`. Force
/// Scream does nothing to a target that cannot be made to cover its ears.
/// Force Suppression and Force Breach do nothing to a target with no
/// running power that the effect ends (`effects::dispel_ends`, the same
/// test the effect uses). Beast Trick and Beast Confusion work only on a
/// beast. Every other row may do something.
pub fn does_something(row: u16, mark: Mark) -> bool {
    use crate::combat::effects::dispel_ends;
    use crate::combat::force;
    use crate::combat::spells::{
        FORCE_POWER_BEAST_CONFUSION, FORCE_POWER_BEAST_TRICK, FORCE_POWER_FORCE_BREACH,
        FORCE_POWER_SUPRESS_FORCE,
    };
    if force::crowd::scream(row) {
        return !mark.deaf;
    }
    let ends = |all: bool| mark.buff.is_some_and(|weakest| dispel_ends(all, weakest));
    match row {
        FORCE_POWER_SUPRESS_FORCE => ends(false),
        FORCE_POWER_FORCE_BREACH => ends(true),
        FORCE_POWER_BEAST_TRICK | FORCE_POWER_BEAST_CONFUSION => mark.beast,
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

/// What a character sees of its target that decides its feat line
/// (`ai.choices.feats`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FeatSight {
    /// The target holds its block now, or holds a lit lightsaber.
    pub blocks: bool,
    /// The target is tired. Only the Advanced rules make a fighter tired.
    pub tired: bool,
    pub staggered: bool,
    /// The target charges at the character, or winds up a charge at it.
    pub charging: bool,
    /// The target stands still.
    pub still: bool,
    /// How far away the target stands (metres).
    pub distance_m: f32,
}

/// Why a character takes a feat line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatWhy {
    Blocks,
    Tired,
    Staggered,
    /// No other reason holds. The medium style and Critical Strike need none.
    Plain,
    Close,
    Charging,
    FarAndStill,
    NotBlocking,
}

impl FeatWhy {
    /// The reason as the choice log writes it.
    pub fn text(self) -> &'static str {
        match self {
            FeatWhy::Blocks => "the target blocks often",
            FeatWhy::Tired => "the target is tired",
            FeatWhy::Staggered => "the target is staggered",
            FeatWhy::Plain => "no other line has a reason",
            FeatWhy::Close => "the target is close",
            FeatWhy::Charging => "the target charges at the character",
            FeatWhy::FarAndStill => "the target is far away and stands still",
            FeatWhy::NotBlocking => "the target does not block",
        }
    }
}

/// The melee lines in the order the character prefers them when their
/// reasons hold. Critical Strike always has a reason, so it comes last.
const MELEE_ORDER: [FeatLine; 3] = [
    FeatLine::Flurry,
    FeatLine::PowerAttack,
    FeatLine::CriticalStrike,
];
/// The gun lines in the order the character prefers them when their
/// reasons hold.
const GUN_ORDER: [FeatLine; 3] = [
    FeatLine::RapidShot,
    FeatLine::SniperShot,
    FeatLine::PowerBlast,
];
/// The gun lines in the order a gunner falls back on them when no gun
/// line's reason holds. Power Blast comes first, as Critical Strike is the
/// melee line that needs no reason.
const GUN_FALLBACK: [FeatLine; 3] = [
    FeatLine::PowerBlast,
    FeatLine::RapidShot,
    FeatLine::SniperShot,
];

/// Why `line` suits a target the character sees as `sight`, if it does.
/// Flurry suits a staggered or tired target. Power Attack suits a target
/// that blocks often. Critical Strike suits any target. Rapid Shot suits a
/// target that charges at the character or stands within
/// `ai.choices.feats.close_m` (5 meters). Sniper Shot suits a target that
/// stands still at least `ai.choices.feats.far_m` (15 meters) away. Power
/// Blast suits a target that does not block.
pub fn feat_reason(line: FeatLine, sight: &FeatSight) -> Option<FeatWhy> {
    let when = |holds: bool, why: FeatWhy| holds.then_some(why);
    match line {
        FeatLine::Flurry => {
            when(sight.staggered, FeatWhy::Staggered).or_else(|| when(sight.tired, FeatWhy::Tired))
        }
        FeatLine::PowerAttack => when(sight.blocks, FeatWhy::Blocks),
        FeatLine::CriticalStrike => Some(FeatWhy::Plain),
        FeatLine::RapidShot => when(sight.charging, FeatWhy::Charging)
            .or_else(|| when(sight.distance_m <= feats::CLOSE_M, FeatWhy::Close)),
        FeatLine::SniperShot => when(
            sight.still && sight.distance_m >= feats::FAR_M,
            FeatWhy::FarAndStill,
        ),
        FeatLine::PowerBlast => when(!sight.blocks, FeatWhy::NotBlocking),
    }
}

/// The feat line a character takes against a target it sees as `sight`,
/// and why. A gunner (`gun`) takes a gun line, any other fighter a melee
/// line. It is the first line in the order of preference that the
/// character `knows` and whose reason holds. When no gun line's reason
/// holds, a gunner takes the first line it knows in the order Power Blast,
/// Rapid Shot, Sniper Shot, for the reason `FeatWhy::Plain`. A target with
/// a lit lightsaber blocks, so without this a gunner that knows only Power
/// Blast would never use it against a Jedi. None when the character knows
/// no line of the kind.
pub fn feat_line(
    sight: &FeatSight,
    gun: bool,
    knows: impl Fn(FeatLine) -> bool,
) -> Option<(FeatLine, FeatWhy)> {
    let order = if gun { GUN_ORDER } else { MELEE_ORDER };
    let reasoned = order
        .into_iter()
        .filter(|line| knows(*line))
        .find_map(|line| Some((line, feat_reason(line, sight)?)));
    let fallback = || {
        (GUN_FALLBACK.into_iter())
            .find(|line| gun && knows(*line))
            .map(|line| (line, FeatWhy::Plain))
    };
    reasoned.or_else(fallback)
}

/// Why a gunner that `knows` some gun lines fires the special of `line`,
/// the line in use, at a target it sees as `sight`. It fires when the
/// line's own reason holds (`feat_reason`), or when the line is the one it
/// falls back on (`feat_line`). None when it does not fire.
pub fn fire_reason(
    line: FeatLine,
    sight: &FeatSight,
    knows: impl Fn(FeatLine) -> bool,
) -> Option<FeatWhy> {
    feat_reason(line, sight).or_else(|| {
        let fallback = feat_line(sight, true, knows) == Some((line, FeatWhy::Plain));
        fallback.then_some(FeatWhy::Plain)
    })
}

/// The saber style a single blade fights in for the melee line `line`.
/// Power Attack is the strong style, Critical Strike the medium style and
/// Flurry the fast style. A gun line has none.
pub fn style_of(line: FeatLine) -> Option<Style> {
    crate::combat::picks::style_of(line)
}

/// Whether a style or line that the chooser changed at `changed_at` (sim
/// ms, 0 for never) is still kept at `now`. It is kept for
/// `ai.choices.feats.hold_ms` (6 seconds) after the change.
pub fn feat_kept(now: u64, changed_at: u64) -> bool {
    changed_at != 0 && now < changed_at.saturating_add(feats::HOLD_MS)
}

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

/// The strength of one side. Each character's level is multiplied by the
/// part of its hit points it has left, and the results are added up. A Boss counts
/// `ai.choices.intense_boss_factor` times (2 times) as much.
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
/// A companion never uses the last `ai.choices.keep_items` items (2 items)
/// of each kind.
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

/// The kinds of use that the characters of one side wait for each other
/// to use (`ai.choices.side_wait_ms`). `Mind::used_at` is indexed by them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideKind {
    Grenade,
    /// A Force power that hits an area.
    Area,
    Disable,
    /// A healing power or a medpac.
    Heal,
    /// A buff, a stim or a shield.
    Buff,
}

impl SideKind {
    pub const ALL: &[SideKind] = &[
        SideKind::Grenade,
        SideKind::Area,
        SideKind::Disable,
        SideKind::Heal,
        SideKind::Buff,
    ];
}

/// The kind of `choice` that a side waits for. A grenade is a grenade
/// whatever its row. A power or an item has the kind the rulebook gives its
/// row (`ai.choices.kinds`). Only shared intense-fight buffs (`Reason::Intense`)
/// wait on allies; personal buffs, stims and energy shields do not. A power
/// that damages a single target, and a row the rulebook does not list, such
/// as a mod's, have none.
pub fn side_kind(choice: Choice) -> Option<SideKind> {
    let row = match choice {
        Choice::Grenade(_) => return Some(SideKind::Grenade),
        Choice::Power(row) | Choice::Item(row) => row,
    };
    match kind_of_row(row)?.0 {
        Kind::AreaDamage => Some(SideKind::Area),
        Kind::Disable => Some(SideKind::Disable),
        Kind::Heal => Some(SideKind::Heal),
        Kind::Buff
            if matches!(choice, Choice::Power(_)) && reason_of(choice) == Reason::Intense =>
        {
            Some(SideKind::Buff)
        }
        Kind::Buff | Kind::Damage => None,
    }
}

/// How long a character waits after an ally it can see used `kind`
/// (`ai.choices.side_wait_ms`).
pub fn side_wait_ms(kind: SideKind) -> u64 {
    use rules::ai::choices::side_wait_ms;
    match kind {
        SideKind::Grenade => side_wait_ms::GRENADE,
        SideKind::Area => side_wait_ms::AREA,
        SideKind::Disable => side_wait_ms::DISABLE,
        SideKind::Heal => side_wait_ms::HEAL,
        SideKind::Buff => side_wait_ms::BUFF,
    }
}

/// Whether a character may use `kind` at `now`, when the latest use of that
/// kind by an ally it can see was at `last_by_an_ally` (sim ms). It may when
/// no ally used it, or when the kind's wait has passed since.
pub fn side_allows(kind: SideKind, now: u64, last_by_an_ally: Option<u64>) -> bool {
    last_by_an_ally.is_none_or(|at| now >= at.saturating_add(side_wait_ms(kind)))
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
