//! The fights of a playthrough beside what KOTOR's own rules forecast for
//! them (`facts.json`'s `fights`): the balance bench.
//!
//! Every weapon hit between the party and a creature outside it is kept
//! with KOTOR's expected damage for one round of that attacker on that
//! target (`round`): for each attack KOTOR gives it in a round (`attacks`),
//! the chance its attack roll hits times its mean damage. Hits less than
//! `GAP_MS` apart make one fight. For each fight the forecast says how many
//! rounds the party needs to take the enemies' vitality and what the
//! enemies deal in that time, falling one by one; the measure says what the
//! party lost and how long the fight took. A fight where the two differ
//! widely is one to explain.
//!
//! The attacks of a round, for both sides alike: one with the main hand;
//! one with the off hand of a second weapon or a double-bladed one, each at
//! two-weapon fighting's penalty (`passive::hand_attack`); one more with
//! the main hand for Flurry (a blade) or Rapid Shot (a gun) where the
//! character knows the feat, every attack of the round at the feat's
//! penalty; and Knight Speed's one or Master Speed's two for a character
//! that knows the power, counted as cast.
//!
//! Left out of the forecast: Force powers that deal damage, stun or heal;
//! grenades, mines, medpacs and energy shields; Power Attack, Critical
//! Strike and their gun counterparts (the round is always Flurry's or Rapid
//! Shot's); the Defense penalty of Flurry and Rapid Shot; the bonuses of
//! powers and stims not running at the moment of the hit; party members
//! falling during the fight; and time spent closing the distance. A
//! double-bladed weapon's second attack takes the main hand's penalty.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use sim::combat::damage;
use sim::combat::passive::{self, Game};
use sim::combat::sheet::{build, feats, flags};
use sim::combat::spells::{FORCE_POWER_KNIGHT_SPEED, FORCE_POWER_SPEED_MASTERY};
use sim::combat::{Outcome, Sheet, Weapon};
use sim::{Kind, Snapshot};

/// Hits further apart than this belong to different fights (ms).
const GAP_MS: u64 = 10_000;
/// Rolls drawn for a forecast's mean damage.
const SAMPLES: u32 = 256;
/// KOTOR's round (s).
const ROUND_S: f32 = 3.0;

/// The attacks KOTOR gives `attacker` in a round (the module's own words
/// say which): each one's weapon and what the round takes from or adds to
/// its attack roll. `scaled` is what a scaled enemy's hit makes of a
/// weapon.
fn attacks(attacker: &Sheet, scaled: &dyn Fn(Weapon) -> Weapon) -> Vec<(Weapon, i32)> {
    let main = scaled(attacker.weapons[0].unwrap_or(attacker.main_weapon()));
    let off = match attacker.weapons {
        [_, Some(off)] => Some(scaled(off)),
        [Some(double), None] if double.build & build::DOUBLE != 0 => Some(main),
        _ => None,
    };
    // The round's feat attack: Flurry's with a blade, Rapid Shot's with a
    // gun, at the penalty of the tier known.
    let feat = if main.delivery.ranged() {
        let tier = usize::from(attacker.feats.count(&feats::RAPID_SHOT));
        (tier > 0).then(|| i32::from(rules::feats::specials::rapid_shot::PENALTY[tier.min(3) - 1]))
    } else {
        let tier = usize::from(attacker.feats.count(&feats::FLURRY));
        (tier > 0).then(|| -i32::from(rules::saber_moves::styles::FLURRY_PENALTY[tier.min(2) - 1]))
    };
    let speed = if attacker.powers.has(FORCE_POWER_SPEED_MASTERY) {
        sim::combat::effects::SPEED_ATTACKS[2]
    } else if attacker.powers.has(FORCE_POWER_KNIGHT_SPEED) {
        sim::combat::effects::SPEED_ATTACKS[1]
    } else {
        0
    };
    let penalty = feat.unwrap_or(0);
    let main_hand = passive::hand_attack(attacker, 0) + penalty;
    let mut all = vec![(main, main_hand)];
    if let Some(off) = off {
        all.push((off, passive::hand_attack(attacker, 1) + penalty));
    }
    let more = usize::from(feat.is_some()) + usize::try_from(speed).unwrap_or(0);
    all.extend(std::iter::repeat_n((main, main_hand), more));
    all
}

/// KOTOR's expected damage for one attack of `attacker` on `target` with
/// `weapon` at `modifier` to its attack roll (whole points): the chance the
/// roll hits (`damage::margin`) times the mean of its damage rolls through
/// the target's guards, criticals among them.
fn attack(attacker: &Sheet, weapon: &Weapon, modifier: i32, target: &Sheet, game: Game) -> f32 {
    // A fixed stream: the same forecast for the same sheets, every run.
    let state = Cell::new(0x9E37_79B9_7F4A_7C15_u64);
    let draw = |sides: u8| {
        let next = state
            .get()
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        state.set(next);
        ((next >> 33) % u64::from(sides.max(1))) as i64 + 1
    };
    let odds = damage::odds(attacker, weapon, target);
    let odds = (odds.0 + modifier, odds.1);
    let confirm = damage::confirming(attacker, odds);
    let total: i64 = (0..SAMPLES)
        .map(|_| {
            let roll = damage::weapon_roll(weapon, Some(attacker), &mut |sides| draw(sides));
            let crit = damage::critical(draw(20), weapon, target, Some(confirm), &mut || draw(20));
            let dealt = damage::weapon_damage(
                weapon,
                Some(attacker),
                target,
                roll,
                crit,
                0,
                game,
                &mut |sides| draw(sides),
            );
            dealt.0.max(0)
        })
        .sum();
    let mean = total as f32 / SAMPLES as f32;
    mean * damage::margin(odds.0, odds.1) as f32 / 1000.0
}

/// KOTOR's expected damage for one round of `attacker` striking `target`
/// (whole points), and the attacks the round has (`attacks`).
pub fn round(
    attacker: &Sheet,
    target: &Sheet,
    game: Game,
    scaled: &dyn Fn(Weapon) -> Weapon,
) -> (f32, usize) {
    let all = attacks(attacker, scaled);
    let dealt = all
        .iter()
        .map(|(weapon, modifier)| attack(attacker, weapon, *modifier, target, game))
        .sum();
    (dealt, all.len())
}

/// One weapon hit across the two sides.
struct Hit {
    at_ms: u64,
    by: u32,
    on: u32,
    by_party: bool,
    /// Vitality taken (whole points).
    damage: f32,
    /// KOTOR's forecast for a round of this attacker on this target, and
    /// the round's attacks.
    forecast: f32,
    attacks: usize,
    /// The same for the target striking back at the attacker: what stands
    /// for a fighter that was hit and landed no hit of its own.
    back: f32,
    by_vitality: u16,
    on_vitality: u16,
    /// The attacker is a scaled enemy; it fires a gun.
    by_scaled: bool,
    by_ranged: bool,
    /// The attacker's blows count ten times over (`--mighty`).
    mighty: bool,
    pc_level: i32,
}

/// The hits of a playthrough, as they land.
#[derive(Default)]
pub struct Ledger {
    seen: BTreeSet<(u32, u16)>,
    hits: Vec<Hit>,
}

/// Whether `entity` fights on the party's side.
fn of_party(entity: &sim::EntityState) -> bool {
    sim::party_member(entity) || !matches!(entity.kind, Kind::Object(..))
}

/// `entity`'s sheet: the one a snapshot carries (a player's, or a
/// creature's once it is its own), else a creature's from its module's
/// facts, as the sim reads it.
fn sheet_of(entity: &sim::EntityState, objects: &sim::Instances) -> Sheet {
    entity.sheet.unwrap_or_else(|| match entity.kind {
        Kind::Object(_, index) => objects
            .facts(index)
            .and_then(|facts| facts.sheet)
            .unwrap_or_else(Sheet::commoner),
        _ => Sheet::commoner(),
    })
}

impl Ledger {
    /// The hits `snapshot` shows that no earlier one did.
    pub fn step(&mut self, world: &sim::SimWorld, snapshot: &Snapshot, game: Game) {
        let level = world.level();
        let mut objects = None;
        for entity in &snapshot.entities {
            let Some(fighter) = entity.fighter else {
                continue;
            };
            let on_party = of_party(entity);
            for event in fighter.events.iter().flatten() {
                if !matches!(event.result, Outcome::Hit | Outcome::Critical) || event.weapon.is_none() {
                    continue;
                }
                let Some(by) = event.by.and_then(|by| snapshot.get(by)) else {
                    continue;
                };
                let by_party = of_party(by);
                if by.fighter.is_none()
                    || by_party == on_party
                    || !self.seen.insert((entity.id.0, event.sequence))
                {
                    continue;
                }
                let objects =
                    objects.get_or_insert_with(|| sim::Instances::in_snapshot(level, snapshot));
                let (sheet, by_sheet) = (sheet_of(entity, objects), sheet_of(by, objects));
                let pc_level = world.script_state().pc_level();
                // A scaled enemy's dice, as its hit multiplies them.
                let set_of = |fighter: &sim::EntityState| match fighter.kind {
                    Kind::Object(_, index) if !of_party(fighter) => objects
                        .facts(index)
                        .zip(level.scripts())
                        .and_then(|(facts, library)| {
                            library.experience().balance(facts.multiplier_set).copied()
                        }),
                    _ => None,
                };
                let scaled = |set: Option<sim::combat::balance::Balance>| {
                    move |weapon: Weapon| set.map_or(weapon, |set| set.weapon(weapon, pc_level))
                };
                let by_scaled = set_of(by).is_some();
                let (forecast, attacks) = round(&by_sheet, &sheet, game, &scaled(set_of(by)));
                let (back, _) = round(&sheet, &by_sheet, game, &scaled(set_of(entity)));
                self.hits.push(Hit {
                    at_ms: snapshot.time_ms,
                    by: by.id.0,
                    on: entity.id.0,
                    by_party,
                    damage: event.damage as f32 / 100.0,
                    forecast,
                    attacks,
                    back,
                    by_vitality: by_sheet.max_hp,
                    on_vitality: sheet.max_hp,
                    by_scaled,
                    by_ranged: event.weapon.is_some_and(|delivery| delivery.ranged()),
                    mighty: by_party && by_sheet.flags & flags::MIGHTY != 0,
                    pc_level,
                });
            }
        }
    }

    /// The fights, each measured and forecast.
    pub fn facts(&self) -> Value {
        let mut fights: Vec<&[Hit]> = Vec::new();
        let mut from = 0;
        for at in 1..=self.hits.len() {
            let ends = self
                .hits
                .get(at)
                .is_none_or(|hit| hit.at_ms > self.hits[at - 1].at_ms + GAP_MS);
            if ends {
                fights.push(&self.hits[from..at]);
                from = at;
            }
        }
        Value::Array(fights.into_iter().filter(|hits| !hits.is_empty()).map(fight).collect())
    }
}

/// One side of a fight: its members' vitality, each attacker's summed
/// forecast for a round with its hits and its round's attacks, and what
/// each member hit would deal striking back (for those that landed none).
#[derive(Default)]
struct Side {
    vitality: BTreeMap<u32, u16>,
    rounds: BTreeMap<u32, (f32, u32, usize)>,
    back: BTreeMap<u32, (f32, u32)>,
    dealt: f32,
}

impl Side {
    fn vitality(&self) -> f32 {
        self.vitality.values().map(|v| f32::from(*v)).sum()
    }

    /// What the side deals in a round by KOTOR's rules, all standing: each
    /// member's mean forecast over its hits, or over the hits it took if
    /// it landed none.
    fn per_round(&self) -> f32 {
        let landed: f32 = self.rounds.values().map(|(sum, hits, _)| sum / *hits as f32).sum();
        let silent: f32 = self
            .back
            .iter()
            .filter(|(id, _)| !self.rounds.contains_key(id))
            .map(|(_, (sum, hits))| sum / *hits as f32)
            .sum();
        landed + silent
    }

    /// The attacks its members that landed a hit have in a round, summed.
    fn attacks(&self) -> usize {
        self.rounds.values().map(|(_, _, attacks)| attacks).sum()
    }
}

/// A fight's facts.
fn fight(hits: &[Hit]) -> Value {
    let (mut party, mut enemies) = (Side::default(), Side::default());
    // The enemies that landed a hit: the scaled ones, and those with guns.
    let (mut scaled, mut gunners) = (BTreeSet::new(), BTreeSet::new());
    for hit in hits {
        let (theirs, target) = if hit.by_party {
            (&mut party, &mut enemies)
        } else {
            (&mut enemies, &mut party)
        };
        theirs.vitality.insert(hit.by, hit.by_vitality);
        target.vitality.insert(hit.on, hit.on_vitality);
        let round = theirs.rounds.entry(hit.by).or_insert((0.0, 0, hit.attacks));
        round.0 += hit.forecast;
        round.1 += 1;
        let back = target.back.entry(hit.on).or_insert((0.0, 0));
        back.0 += hit.back;
        back.1 += 1;
        theirs.dealt += hit.damage;
        if !hit.by_party && hit.by_scaled {
            scaled.insert(hit.by);
        }
        if !hit.by_party && hit.by_ranged {
            gunners.insert(hit.by);
        }
    }
    let seconds = (hits[hits.len() - 1].at_ms - hits[0].at_ms) as f32 / 1000.0;
    let (ours, theirs) = (party.vitality().max(1.0), enemies.vitality());
    // KOTOR's forecast: the rounds the party needs for the enemies'
    // vitality; the enemies deal their round's damage meanwhile, fewer of
    // them each time one falls.
    let foes = enemies.vitality.len().max(1) as f32;
    let rounds = (party.per_round() > 0.0).then(|| theirs / party.per_round());
    let lost = rounds.map(|rounds| enemies.per_round() * rounds * (foes + 1.0) / (2.0 * foes));
    json!({
        "at_s": hits[0].at_ms as f32 / 1000.0,
        "pc_level": hits[0].pc_level,
        "mighty": hits.iter().any(|hit| hit.mighty),
        "party": party.vitality.len(),
        "enemies": enemies.vitality.len(),
        "party_vitality": ours,
        "enemy_vitality": theirs,
        "enemies_hitting": enemies.rounds.len(),
        "scaled_hitting": scaled.len(),
        "gunners_hitting": gunners.len(),
        "kjk": {
            "seconds": seconds,
            "party_lost": enemies.dealt,
            "lost_share": enemies.dealt / ours,
            "party_dealt": party.dealt,
        },
        "kotor": {
            "party_per_round": party.per_round(),
            "party_attacks": party.attacks(),
            "enemy_per_round": enemies.per_round(),
            "enemy_attacks": enemies.attacks(),
            "seconds": rounds.map(|rounds| rounds * ROUND_S),
            "party_lost": lost,
            "lost_share": lost.map(|lost| lost / ours),
        },
    })
}
