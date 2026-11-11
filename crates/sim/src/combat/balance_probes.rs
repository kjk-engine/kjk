//! KOTOR II's enemy scaling (`combat::balance`), for `kjk sim probes`: the
//! sets' arithmetic as TSL's executable works it, and a creature a module
//! places scaled as it comes to stand on the proving ground.

use super::balance::Balance;
use super::{Sheet, Weapon};
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::script::xp::{Experience, PC_LEVEL};
use crate::{Command, Kind, Level, Seat, SimWorld, Snapshot, StepReason, TickInput, WorldConfig};

/// TSL's own `Set_3` and `Set_5`, as its `autobalance.2da` has them.
const SET_3: Balance = Balance {
    vitality: 0.65,
    to_hit: 0.95,
    armor: 0.80,
    damage: 0.11,
    save: 1.0,
    level: 0.75,
};
const SET_5: Balance = Balance {
    vitality: 0.80,
    to_hit: 1.1,
    armor: 1.00,
    damage: 0.175,
    save: 1.05,
    level: 0.75,
};

/// What scaling moves on a sheet: most vitality, base attack bonus,
/// defence, the saves and the level.
type Moved = (u16, u8, i8, [i8; 3], u8);

fn moved(sheet: &Sheet) -> Moved {
    (
        sheet.max_hp,
        sheet.bab,
        sheet.defence,
        sheet.saves,
        sheet.level,
    )
}

/// A level 1 creature as TSL's templates often are: 20 vitality, base
/// attack bonus 1, defence 2, saves 1, 2 and 3.
fn template() -> Sheet {
    Sheet {
        max_hp: 20,
        bab: 1,
        defence: 2,
        saves: [1, 2, 3],
        level: 1,
        ..Sheet::commoner()
    }
}

/// (name, the set, the main character's level, what the template becomes).
/// At 15 with `Set_3` vitality is 9 times the template's (1 and 13 x 0.65
/// rounded down), the attack bonus gains 14, defence 12, each save 15 and
/// the level 11 less 1. At 0 the game has told no level: it counts as 1
/// for all but vitality, and the level comes to 0 as the executable leaves
/// it. At 2 vitality has not yet grown.
const SCALED: [(&str, Balance, i32, Moved); 4] = [
    (
        "balance_set_3_at_15",
        SET_3,
        15,
        (180, 15, 14, [16, 17, 18], 11),
    ),
    ("balance_level_untold", SET_3, 0, (20, 1, 2, [2, 3, 4], 0)),
    ("balance_set_3_at_2", SET_3, 2, (20, 2, 3, [3, 4, 5], 1)),
    (
        "balance_set_5_at_30",
        SET_5,
        30,
        (460, 34, 32, [32, 33, 34], 22),
    ),
];

/// (the set, the main character's level, how many times the dice count).
const TIMES: [(Balance, i32, u8); 6] = [
    (SET_3, 0, 1),
    (SET_3, 1, 1),
    (SET_3, 10, 1),
    (SET_3, 11, 2),
    (SET_5, 15, 3),
    (SET_5, 30, 6),
];

pub fn probes() -> Checks {
    let mut list: Checks = SCALED
        .iter()
        .map(|(name, set, level, wanted)| {
            let seen = moved(&set.scaled(template(), *level));
            let what = format!("level {level}: {seen:?}, wanted {wanted:?}");
            (*name, check(seen == *wanted, what))
        })
        .collect();
    list.push(("balance_damage_times", damage_times()));
    list.push(("balance_no_set", no_set()));
    list.extend(stood().unwrap_or_else(|why| vec![("balance_scenario", Err(why))]));
    list
}

/// The dice of a weapon hit: 1 + (level - 1) x the set's damage multiplier,
/// rounded down before the 1; the weapon's bonus dice stay.
fn damage_times() -> Result<String, String> {
    let seen: Vec<u8> = TIMES
        .iter()
        .map(|(set, level, _)| set.damage_times(*level))
        .collect();
    let wanted: Vec<u8> = TIMES.iter().map(|(.., times)| *times).collect();
    let claws = Weapon {
        dice: 1,
        die: 6,
        bonus_dice: 2,
        ..Weapon::UNARMED
    };
    let grown = SET_5.weapon(claws, 15);
    check(
        seen == wanted && (grown.dice, grown.die, grown.bonus_dice) == (3, 6, 2),
        format!(
            "{seen:?}, wanted {wanted:?}; 1d6 with 2 bonus dice at 15: {}d{} with {}",
            grown.dice, grown.die, grown.bonus_dice
        ),
    )
}

/// Set 0 is no scaling, and a row the table lacks is none.
fn no_set() -> Result<String, String> {
    let experience = tables();
    let found = [0, 1, 2].map(|set| experience.balance(set).is_some());
    check(found == [false, true, false], format!("{found:?}"))
}

/// The tables of a game with one scaling set, `Set_3`, in row 1.
fn tables() -> Experience {
    Experience {
        balance: vec![Balance::default(), SET_3],
        ..Experience::default()
    }
}

/// The foes' tags and template: one scaled by set 1, one with no set.
const SCALED_FOE: &str = "probe_scaled";
const PLAIN_FOE: &str = "probe_plain";

/// The proving ground as module `name` with `foes` placed behind its entry
/// (each a tag and its scaling set), its library holding the tables.
fn ground(name: &str, foes: &[(&str, u8)]) -> Result<Level, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.name = name.into();
    let [x, y, z] = data.entry.position;
    let foe = |n: usize, tag: &str, set: u8| ObjectData {
        multiplier_set: set,
        ..ObjectData::creature(tag, tag, [x - 2.0 * (n + 1) as f32, y, z], template())
    };
    data.placements.extend(
        foes.iter()
            .enumerate()
            .map(|(n, (tag, set))| Placement::Own(foe(n, tag, *set))),
    );
    let library =
        crate::script::Library::new(Default::default(), Default::default(), Default::default())
            .with_experience(tables());
    Ok(ground.with_module(data).with_scripts(library))
}

/// Two ticks with seat 0's idle command.
fn drive(world: &mut SimWorld) -> Snapshot {
    let from = world.snapshot().tick;
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    crate::step(world, from + 1, &input, 8, StepReason::Authority);
    crate::step(world, from + 2, &input, 8, StepReason::Authority)
}

/// The creature tagged `tag` in `snapshot`: what scaling moved on the sheet
/// it carries (none: its template's own) and its vitality (hundredths).
fn foe(level: &Level, snapshot: &Snapshot, tag: &str) -> Option<(Option<Moved>, i32)> {
    let module = level.module()?;
    snapshot.entities.iter().find_map(|entity| {
        let Kind::Object(ObjectKind::Creature, index) = entity.kind else {
            return None;
        };
        let named = module.object(index as usize)?.tag == tag;
        let fighter = entity.fighter.as_ref().filter(|_| named)?;
        Some((entity.sheet.as_ref().map(moved), fighter.hp))
    })
}

/// The main character reaches level 15 in `probe_a`, then the party enters
/// `probe_b`, which places a foe of set 1 and a foe of no set. The scaled
/// foe stands with `Set_3`'s numbers for level 15 and whole; the plain one
/// keeps its template's. The scaled sheet survives the snapshot's encoding.
/// A level told later moves neither.
fn stood() -> Result<Checks, String> {
    let error = |error: crate::InstallError| error.to_string();
    let mut world = SimWorld::new(WorldConfig::one_seat(), ground("probe_a", &[])?)
        .map_err(|error| error.to_string())?;
    drive(&mut world);
    let mut state = world.script_state().clone();
    state.numbers.insert(PC_LEVEL.to_string(), 15);
    world.set_script_state(state);
    let level = ground("probe_b", &[(SCALED_FOE, 1), (PLAIN_FOE, 0)])?;
    world.install(level.clone(), None).map_err(error)?;
    let entered = drive(&mut world);
    let wanted = Some((Some((180, 15, 14, [16, 17, 18], 11)), 18_000));
    let scaled = foe(&level, &entered, SCALED_FOE);
    let plain = foe(&level, &entered, PLAIN_FOE);
    let decoded = Snapshot::decode(&entered.encode()).map_err(|error| error.to_string())?;
    let kept = foe(&level, &decoded, SCALED_FOE);
    let mut state = world.script_state().clone();
    state.numbers.insert(PC_LEVEL.to_string(), 20);
    world.set_script_state(state);
    let later = foe(&level, &drive(&mut world), SCALED_FOE);
    Ok(vec![
        (
            "balance_placed_scaled",
            check(scaled == wanted, format!("{scaled:?}, wanted {wanted:?}")),
        ),
        (
            "balance_placed_plain",
            check(
                plain == Some((None, 2_000)),
                format!("{plain:?}: its template's sheet, 20 vitality"),
            ),
        ),
        (
            "balance_encodes",
            check(kept == wanted, format!("decoded: {kept:?}")),
        ),
        (
            "balance_keeps_spawn_level",
            check(later == wanted, format!("level 20 told later: {later:?}")),
        ),
    ])
}
