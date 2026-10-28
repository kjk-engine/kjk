//! `kjk combat damage`: the damage pipeline's type step without a window
//! (`docs/COMBAT.md`; `sim::combat::damage`,
//! `Sheet::guards`, `Weapon::versus`). Checks the rule's cases, then
//! measures `damage::assess` over many seeded rolls: a bonus against a race
//! lands on that race only, immunity and resistance take their part (all of
//! it: no minimum hundredth), a weapon of two types hurts as the one its
//! target guards least; then the module's creatures whose items guard them
//! (read from the game, `session::sheets`), shot with a disruptor (energy)
//! and a sonic pistol; and a player's sheet with guards and a race bonus
//! through a snapshot's bytes. Writes `facts.json`.

use std::process::ExitCode;

use sim::combat::damage::{self, Blow, FriendlyFire, Relation, Source};
use sim::combat::gear::{Against, Roll};
use sim::combat::sheet::{DAMAGE_TYPES, Guards, Versus, Weapon, damage_types, feats, races};
use sim::combat::{Location, Sheet};
use sim::module::ObjectKind;
use sim::{SimRng, SimWorld, Snapshot};
use vfs::Discovery;

use crate::scenes::Report;

/// Damage type rows (`iprp_damagetype.2da`).
const SLASHING: usize = 2;
const SONIC: usize = 10;
const ION: usize = 11;
const ENERGY: usize = 12;

/// Rolls `damage::assess` of `weapon` from the dev soldier on `target`
/// `rolls` times (a 1 s cadence); the mean (HP) and how many dealt nothing.
fn measure(weapon: Weapon, target: &Sheet, rolls: u64) -> (f64, u64) {
    mean_hit(&Sheet::dev_soldier(), weapon, target, 1000, rolls)
}

/// Rolls `damage::assess` of `weapon` in `attacker`'s hands at `cadence_ms`
/// on `target` `rolls` times (the chest, hostile, seeded by the roll's
/// number): the mean (HP) and how many dealt nothing.
pub fn mean_hit(
    attacker: &Sheet,
    weapon: Weapon,
    target: &Sheet,
    cadence_ms: u32,
    rolls: u64,
) -> (f64, u64) {
    let mut total = 0i64;
    let mut nothing = 0;
    for roll in 0..rolls {
        let blow = Blow {
            attacker: None,
            relation: Relation::Hostile,
            source: Source::Weapon {
                weapon,
                cadence_ms,
                share: 1000,
                area: false,
            },
            location: Location::Chest,
            direction: None,
            sneak: 0,
        };
        let assessed = damage::assess(
            &blow,
            Some(attacker),
            target,
            FriendlyFire::Off,
            sim::combat::passive::Game::K1,
            &mut SimRng::new(roll, 7),
        );
        total += i64::from(assessed.damage);
        nothing += u64::from(assessed.damage == 0);
    }
    (total as f64 / rolls as f64 / 100.0, nothing)
}

fn guarded(row: usize, immunity: i8, resist: u8) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    sheet.guards.immunity[row] = immunity;
    sheet.guards.resist[row] = resist;
    sheet
}

/// `kjk combat damage`.
pub fn damage(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    outln!("combat damage {module}: the type step (I22 section 1 step 9)");

    outln!("  the rule");
    let mut guards = Guards::default();
    guards.immunity[ENERGY] = 50;
    guards.resist[ENERGY] = 3;
    guards.immunity[SONIC] = -50;
    report.check(
        guards.take(ENERGY, 10) == 2
            && guards.take(ENERGY, 4) == 0
            && guards.take(SONIC, 10) == 15
            && guards.take(SLASHING, 10) == 10,
        "50% immunity then resist 3: 10 → 2, 4 → 0; 50% vulnerability: 10 → 15; unguarded types pass whole".to_string(),
    );
    report.check(
        guards.weakest(damage_types::ENERGY | damage_types::SONIC, 10) == Some(SONIC)
            && guards.weakest(damage_types::ENERGY, 10) == Some(ENERGY)
            && guards.weakest(0, 10).is_none(),
        "a weapon of two types hurts as the one guarded least".to_string(),
    );

    outln!("  assessed (2000 seeded rolls each)");
    let pistol = Weapon::plain(
        12,
        1,
        6,
        (1, 2),
        damage_types::ENERGY,
        [feats::PROFICIENCY_BLASTER, 0, 0],
    );
    let ion = Weapon {
        versus: Some(Versus {
            against: Against::Race(races::DROID),
            attack: 0,
            damage: Roll {
                dice: 2,
                die: 6,
                bonus: 0,
            },
            damage_type: ION as u8,
        }),
        ..pistol
    };
    let human = Sheet::dev_soldier();
    let mut droid = Sheet::dev_soldier();
    droid.race = races::DROID;
    let (plain_human, _) = measure(pistol, &human, 2000);
    let (ion_human, _) = measure(ion, &human, 2000);
    let (ion_droid, _) = measure(ion, &droid, 2000);
    report.check(
        (ion_human - plain_human).abs() < 1e-9 && ion_droid > 2.0 * ion_human,
        format!(
            "+2d6 ion against droids: {ion_droid:.2} HP a hit on a droid, {ion_human:.2} on a human (the plain pistol {plain_human:.2})"
        ),
    );
    let (immune, nothing) = measure(pistol, &guarded(ENERGY, 100, 0), 2000);
    report.check(
        immune == 0.0 && nothing == 2000,
        format!("100% energy immunity: {immune:.2} HP, {nothing} of 2000 hits deal nothing (no minimum hundredth)"),
    );
    let (halved, _) = measure(pistol, &guarded(ENERGY, 50, 0), 2000);
    let (vulnerable, _) = measure(pistol, &guarded(ENERGY, -50, 0), 2000);
    report.check(
        (halved / plain_human - 0.5).abs() < 0.12 && (vulnerable / plain_human - 1.5).abs() < 0.12,
        format!(
            "50% immunity {halved:.2} and 50% vulnerability {vulnerable:.2} against {plain_human:.2} (rounded down per roll)"
        ),
    );
    let (resisted, stopped) = measure(pistol, &guarded(ENERGY, 0, 5), 2000);
    report.check(
        resisted < plain_human * 0.5 && stopped > 500,
        format!(
            "resist 5 against 1d6 energy: {resisted:.2} HP, {stopped} of 2000 hits stopped (KOTOR: all but a 6, and crits)"
        ),
    );
    let both = Weapon {
        damage_types: damage_types::ENERGY | damage_types::SONIC,
        ..pistol
    };
    let (mixed, _) = measure(both, &guarded(ENERGY, 100, 0), 2000);
    report.check(
        (mixed - plain_human).abs() < 1e-9,
        format!("energy-and-sonic on an energy-immune target hurts as sonic: {mixed:.2} HP"),
    );

    outln!("  {module}'s guarded creatures (read from the game)");
    let installer = match crate::scenes::installer(discovery, game) {
        Ok(installer) => installer,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let plan = match installer.prepare_one(module, None) {
        Ok(prepared) => prepared.plan,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let data = plan.level.module().expect("a plan has its module");
    let disruptor = Weapon::plain(
        16,
        2,
        6,
        (1, 2),
        damage_types::ENERGY,
        [feats::PROFICIENCY_BLASTER_RIFLE, 0, 0],
    );
    let sonic = Weapon::plain(
        17,
        1,
        6,
        (1, 2),
        damage_types::SONIC,
        [feats::PROFICIENCY_BLASTER, 0, 0],
    );
    let mut creatures = Vec::new();
    for object in data
        .objects()
        .filter(|object| object.kind == ObjectKind::Creature)
    {
        let Some(sheet) = object
            .sheet
            .filter(|sheet| sheet.guards != Guards::default())
        else {
            continue;
        };
        let (energy, _) = measure(disruptor, &sheet, 1000);
        let (sound, _) = measure(sonic, &sheet, 1000);
        let open = (0..DAMAGE_TYPES)
            .filter(|row| (sheet.guards.immunity[*row], sheet.guards.resist[*row]) != (0, 0))
            .map(|row| {
                format!(
                    "type {row} {}% / {}",
                    sheet.guards.immunity[row], sheet.guards.resist[row]
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        outln!(
            "    {} ({open}): disruptor {energy:.2} HP a hit, sonic pistol {sound:.2}",
            object.tag
        );
        creatures.push(serde_json::json!({
            "tag": object.tag, "guards": open, "disruptor": energy, "sonic": sound,
        }));
    }
    if creatures.is_empty() {
        outln!("    none");
    }

    outln!("  through a snapshot");
    let mut sheet = Sheet::dev_soldier();
    sheet.guards = guards;
    sheet.weapons[0] = Some(ion);
    let config = sim::WorldConfig {
        sheet,
        ..sim::WorldConfig::one_seat()
    };
    let same = SimWorld::new(config, sim::Level::arena())
        .map(|mut world| world.snapshot())
        .ok()
        .and_then(|snapshot| Snapshot::decode(&snapshot.encode()).ok())
        .and_then(|decoded| decoded.entities.iter().find_map(|entity| entity.sheet))
        == Some(sheet);
    report.check(
        same,
        "a player's guards and race bonus come back from the bytes".to_string(),
    );

    report.finish(serde_json::json!({
        "verb": "combat damage", "game": game, "module": module, "creatures": creatures,
    }))
}
