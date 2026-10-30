//! The probes of TSL's creature autobalance (`kjk sim probes "tsl
//! autobalance"`, `combat::balance`), with `autobalance.2da`'s own rows, on
//! the proving ground, no install:
//! - the formulas for a known row and level (`HandleAutoBalancing`'s hit
//!   points, the getters' attack, defence, saves and level, the damage dice
//!   factor);
//! - a TSL creature's spawn: its hit points grown from the template's
//!   `HitPoints` by `VPMult` at `G_PC_LEVEL` then, standing at the new most,
//!   the getters' terms on the sheet the pipeline reads, kept through a
//!   snapshot's bytes;
//! - its death's XP by the level it spawned at, though `G_PC_LEVEL` rose;
//! - a K1 creature of the same template and the player's own character
//!   untouched.

use super::{Fighting, Id, SimWorld};
use crate::combat::balance::{Balance, Multipliers};
use crate::combat::fighter::Life;
use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};
use crate::combat::{Fighter, Sheet};
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::xp::{Experience, PC_LEVEL};
use crate::snapshot::{EntityState, Snapshot};
use crate::{Level, StepReason, WorldConfig};

const TAG: &str = "probe_balanced";

/// `autobalance.2da`'s rows 0 to 5 as TSL ships them (`vpmult`, `tohitmult`,
/// `armormult`, `damagemult`, `savemult`, `crmod`, `levelmult`).
fn rows() -> Vec<Multipliers> {
    [
        (0.0, 0.0, 0.0, 0.0, 0.0, 0, 0.0),
        (0.65, 0.9, 0.6, 0.065, 0.8, -3, 0.75),
        (0.65, 0.9, 0.70, 0.09, 0.9, -2, 0.75),
        (0.65, 0.95, 0.80, 0.11, 1.0, -1, 0.75),
        (0.70, 1.0, 0.90, 0.135, 1.0, 1, 0.75),
        (0.80, 1.1, 1.00, 0.175, 1.05, 4, 0.75),
    ]
    .map(
        |(vp, to_hit, armour, damage, save, crmod, level)| Multipliers {
            vp,
            to_hit,
            armour,
            damage,
            save,
            crmod,
            level,
        },
    )
    .to_vec()
}

/// A death's XP 10 a point of rating at every level (so the row the PC's
/// XP reaches does not matter), the rows above, `GAME_XP_General` 100.
fn experience() -> Experience {
    Experience {
        kills: vec![(0..=30).map(|rating| rating * 10).collect(); 31],
        balance: rows(),
        percent: Some((100, 0)),
        ..Experience::default()
    }
}

/// The template's sheet: level 3, BAB 2, defence 1, saves 3/1/1, a most
/// of 24 hit points of which its `HitPoints` are 20 (4 Constitution's).
fn template_sheet() -> Sheet {
    Sheet {
        level: 3,
        bab: 2,
        defence: 1,
        saves: [3, 1, 1],
        max_hp: 24,
        faction: HOSTILE_1,
        ..Sheet::commoner()
    }
}

/// The proving ground as `game`'s, with a hostile creature of set
/// `set` placed far from the player, entered with `G_PC_LEVEL` at
/// `pc_level`: its first visit, the creatures' spawns handled then.
fn world(game: &str, set: u8, pc_level: i32) -> Result<SimWorld, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.game = game.into();
    data.repute = Repute::at_war([HOSTILE_1, PLAYER]);
    let place = crate::determinism::ground_point(6.0, 4.0, 0.0);
    data.placements.push(Placement::Own(ObjectData {
        multiplier_set: set,
        hit_points: Some((15, 20)),
        ..ObjectData::creature(TAG, TAG, place, template_sheet())
    }));
    let library = crate::probe::library("", Vec::<(String, fmt_odyssey::ncs::Ncs)>::new())
        .with_experience(experience());
    let level: Level = ground.with_module(data).with_scripts(library);
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    // Entered again as a first visit with the global set: the spawn's
    // handling reads it.
    let mut carried = world.take_scripts();
    carried.state.spawned.clear();
    crate::script::set_global(&mut carried.state.numbers, PC_LEVEL, pc_level);
    world.enter_module(carried);
    Ok(world)
}

/// The creature placed, and the player's own character, in `snapshot`.
fn bodies<'a>(
    world: &SimWorld,
    snapshot: &'a Snapshot,
) -> Result<(&'a EntityState, &'a EntityState), String> {
    let objects = crate::Instances::in_snapshot(world.level(), snapshot);
    let creature = (snapshot.entities.iter())
        .find(|e| objects.of_entity(e).is_some_and(|data| data.tag == TAG))
        .ok_or("no creature")?;
    let player = (snapshot.entities.iter())
        .find(|e| e.fighter.is_some() && objects.of_entity(e).is_none())
        .ok_or("no player")?;
    Ok((creature, player))
}

/// Its sheet as the pipeline reads it (`effects::sheet_now`), and its
/// fighter.
fn read(entity: &EntityState) -> Result<(Sheet, Fighter), String> {
    let fighter = entity.fighter.ok_or("no fighter")?;
    let own = entity.sheet.unwrap_or_else(template_sheet);
    Ok((crate::combat::effects::sheet_now(own, &fighter, 0), fighter))
}

fn step(world: &mut SimWorld) {
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, StepReason::Authority);
}

type Case = fn() -> Result<String, String>;

pub fn probes() -> Checks {
    let cases: [(&'static str, Case); 6] = [
        ("formulas", formulas),
        ("weapon_dice_factor", weapon_dice),
        ("tsl_spawn_grows", tsl_spawn),
        ("death_xp_by_the_level_at_spawn", death_xp),
        ("k1_creature_untouched", k1_untouched),
        ("player_untouched", player_untouched),
    ];
    cases.into_iter().map(|(name, run)| (name, run())).collect()
}

/// Set_2 and Set_5 at P 10, Set_1 at P 1 and P 0, in single precision as
/// the executable reckons. Set_2: `ToHitMult` 0.9 × 10, 9; `ArmorMult` 0.70
/// × 10, 7; `SaveMult` 0.9 × 10, 9; `LevelMult` 0.75 × 10 − 1, 6; `DamageMult` 0.09
/// × 9, 0, + 1; `VPMult` 0.65 × 8, 5, + 1: 20 HP are 120. Set_5: 1.1 × 10,
/// 11; 1.0 × 10, 10; 1.05 × 10, 10; 0.175 × 9, 1, + 1: 2; 0.80 × 8, 6, +
/// 1: 140. Set_1 at P 1: 0.9, 0; 0.6, 0; 0.8, 0; 0.75 − 1, −1; the dice
/// ×1; hit points as they are below P 2; at P 0 the getters count 1 and
/// the dice 1.
fn formulas() -> Result<String, String> {
    let rows = rows();
    let term = |balance: Balance| {
        (
            balance.attack,
            balance.armour,
            balance.save,
            balance.level,
            balance.dice,
        )
    };
    let seen = (
        term(rows[2].balance(10)),
        rows[2].hit_points(20, 10),
        term(rows[5].balance(10)),
        rows[5].hit_points(20, 10),
        term(rows[1].balance(1)),
        rows[1].hit_points(20, 1),
        term(rows[1].balance(0)),
    );
    let wanted = (
        (9, 7, 9, 6, 1),
        120,
        (11, 10, 10, 6, 2),
        140,
        (0, 0, 0, -1, 1),
        20,
        (0, 0, 0, -1, 1),
    );
    check(seen == wanted, format!("{seen:?}, wanted {wanted:?}"))
}

/// A weapon roll of 2d6 + 3 whose dice all show 4: 11 at factor 1, 19 at
/// 2 (each die doubled, the flat bonus not: `GetDamageRoll`).
fn weapon_dice() -> Result<String, String> {
    let weapon = crate::combat::Weapon {
        dice: 2,
        die: 6,
        bonus_dice: 0,
        damage_bonus: 3,
        ..crate::combat::Weapon::UNARMED
    };
    let roll = |factor| crate::combat::damage::weapon_roll(&weapon, None, factor, &mut |_| 4);
    let seen = (roll(1), roll(2));
    check(seen == (11, 19), format!("{seen:?}, wanted (11, 19)"))
}

/// A Set_2 creature spawned at `G_PC_LEVEL` 10: its 20 `HitPoints` grow
/// to 120, its most 124 with the 4 of Constitution, and it stands at it
/// (the template's 15 of 20 forgotten); BAB 2 + 9, defence 1 + 7, each
/// save 9 up, level 3 + 6; P 10 and the dice ×1 kept through the
/// snapshot's bytes (a save's).
fn tsl_spawn() -> Result<String, String> {
    let mut world = world("tsl", 2, 10)?;
    let snapshot = world.snapshot();
    let (creature, _) = bodies(&world, &snapshot)?;
    let (now, fighter) = read(creature)?;
    let bytes = snapshot.encode();
    let back = Snapshot::decode(&bytes).map_err(|error| error.to_string())?;
    let (again, _) = bodies(&world, &back)?;
    let kept = again.fighter.and_then(|fighter| fighter.balance);
    let seen = (
        now.max_hp,
        fighter.hp,
        now.bab,
        now.defence,
        now.saves,
        now.level,
        fighter
            .balance
            .map(|balance| (balance.pc_level, balance.dice)),
        kept == fighter.balance,
    );
    let wanted = (124, 12_400, 11, 8, [12, 10, 10], 9, Some((10, 1)), true);
    check(seen == wanted, format!("{seen:?}, wanted {wanted:?}"))
}

/// The Set_2 creature spawned at 8 dies after `G_PC_LEVEL` rose to 12: its
/// rating is 8 − 2, 60 XP (12 − 2 would give 100).
fn death_xp() -> Result<String, String> {
    let mut world = world("tsl", 2, 8)?;
    step(&mut world);
    let mut scripts = world.take_scripts();
    crate::script::set_global(&mut scripts.state.numbers, PC_LEVEL, 12);
    let before = scripts.state.xp(crate::inventory::PARTY);
    world.world.insert_resource(scripts);
    let snapshot = world.snapshot();
    let (creature, _) = bodies(&world, &snapshot)?;
    let id = creature.id;
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    for (of, mut fighting) in fighters.iter_mut(&mut world.world) {
        if of.0 == id {
            fighting.0.hp = 0;
            fighting.0.life = Life::Dead;
        }
    }
    step(&mut world);
    let gained = world.script_state().xp(crate::inventory::PARTY) - before;
    check(gained == 60, format!("{gained} XP, wanted 60"))
}

/// The same template in K1 at `G_PC_LEVEL` 10: K1's executable has no
/// autobalance, so its sheet and fighter are the template's.
fn k1_untouched() -> Result<String, String> {
    let mut world = world("k1", 2, 10)?;
    let snapshot = world.snapshot();
    let (creature, _) = bodies(&world, &snapshot)?;
    let (now, fighter) = read(creature)?;
    let seen = (
        creature.sheet.is_none(),
        now.max_hp,
        fighter.hp,
        now.bab,
        fighter.balance,
    );
    let wanted = (true, 24, 1_900, 2, None);
    check(seen == wanted, format!("{seen:?}, wanted {wanted:?}"))
}

/// The player's own character beside the balanced creature: never
/// balanced, its sheet the one it came with.
fn player_untouched() -> Result<String, String> {
    let mut world = world("tsl", 2, 10)?;
    let snapshot = world.snapshot();
    let (_, player) = bodies(&world, &snapshot)?;
    let fighter = player.fighter.ok_or("no fighter")?;
    let sheet = player.sheet.ok_or("no sheet")?;
    let soldier = Sheet::dev_soldier();
    let seen = (
        fighter.balance,
        sheet.max_hp == soldier.max_hp,
        sheet.bab == soldier.bab,
    );
    let wanted = (None, true, true);
    check(seen == wanted, format!("{seen:?}, wanted {wanted:?}"))
}
