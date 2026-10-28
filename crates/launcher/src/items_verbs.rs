//! `kjk combat items`: KOTOR's consumables without a window (I22 sections
//! 5 and 7; `sim::combat::items`). The table and the healing rule; in the
//! open arena a medpac's use time and its healing, a second and a third
//! press, a droid's repair kit (a medpac is no use to it), an antidote, a
//! stim (and its vitality taken back when it ends), a squad stim healing
//! both seats; in a module an energy shield soaking a hostile gunner's
//! bolts until spent, and a creature using the medpac it carries when hurt,
//! by its own AI scripts. Every scene's recording replays. Writes
//! `facts.json`.

use std::process::ExitCode;

use sim::combat::items::{self, Carried, KINDS, Pack, Poisoned, Stim, Use};
use sim::combat::sheet::{Ability, races};
use sim::combat::spells::*;
use sim::combat::{Fighter, Sheet, mind};
use sim::{Command, SimWorld, Snapshot, buttons};
use vfs::Discovery;

use crate::scenes::{Report, Run, Stage, fighter, player, seat0};

/// The dev soldier with 500 HP carrying `items`.
fn carrying(items: &[(u16, u8)]) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    sheet.max_hp = 500;
    sheet.items = [None; KINDS];
    for (slot, &(row, count)) in sheet.items.iter_mut().zip(items) {
        *slot = Some(Carried { row, count });
    }
    sheet
}

/// Presses `button` at each of `at` (ms after the start) for one step.
fn presses(seat: usize, button: u16, at: &[u64]) -> impl FnMut(&Snapshot, u64) -> Command + '_ {
    move |last, since| {
        let ps = player(last, seat);
        Command {
            buttons: if at.contains(&since) { button } else { 0 },
            ..mind::looking(&ps, 0.0, ps.view[1])
        }
    }
}

/// The HP (whole) of the entity at `at` in the snapshot `ms` after the
/// start.
fn hp_at(run: &Run, at: usize, ms: u64) -> f64 {
    let snapshot = &run.snapshots[(ms / 8).saturating_sub(1) as usize];
    f64::from(fighter(snapshot, at).hp) / 100.0
}

/// A stage of the arena with `sheet` for every seat, at `hp` hundredths.
fn arena(sheet: Sheet, seats: u8, hp: i32) -> (Stage, Snapshot) {
    let config = sim::WorldConfig {
        seats,
        sheet,
        ..sim::WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, sim::Level::arena()).expect("the arena");
    let stage = Stage::of(&mut world);
    let mut start = stage.start.clone();
    for entity in &mut start.entities {
        if let Some(fighter) = entity.fighter.as_mut() {
            fighter.hp = hp;
        }
    }
    (stage, start)
}

/// The install's `spells.2da` labels, by row, as `module` resolves them.
fn spell_labels(discovery: &Discovery, game: &str, module: &str) -> Result<Vec<String>, String> {
    use fmt_odyssey::ResType;
    use fmt_odyssey::twoda::TwoDa;
    let resolver = crate::scenes::installer(discovery, game)?.resolver(module)?;
    let found = resolver
        .find("spells", ResType::from_extension("2da").expect("a known type"))
        .ok_or("spells.2da: not in the install")?;
    let bytes = found.location.read().map_err(|error| error.to_string())?;
    let table = TwoDa::parse(&bytes).map_err(|error| error.to_string())?;
    let label = table.column("label").ok_or("spells.2da: no label")?;
    Ok((0..table.cells.len())
        .map(|row| {
            table
                .get(row, label)
                .map(|cell| String::from_utf8_lossy(cell).trim().to_string())
                .unwrap_or_else(String::new)
        })
        .collect())
}

/// `stage` with the creature at `target` carrying `held` (item templates
/// and stacks) in its inventory, its sheet changed by `change` and its
/// usable kinds those items' (as its goods will have them): the stage made
/// anew for one seat, and the creature's sheet.
pub(crate) fn given_items(
    stage: &Stage,
    target: usize,
    held: &[(&str, u16)],
    change: impl FnOnce(&mut Sheet),
) -> Result<(Stage, Sheet), String> {
    let mut data = stage.level.module().ok_or("no module")?.as_ref().clone();
    let sim::Kind::Object(_, index) = stage.start.entities[target].kind else {
        return Err("the target is no module creature".to_string());
    };
    let library = stage.level.scripts().ok_or("the module has no scripts")?;
    let creature = data
        .object_mut(index as usize)
        .ok_or("the target is not placed")?;
    let mut theirs = creature.sheet.unwrap_or_else(Sheet::commoner);
    change(&mut theirs);
    let catalog = library.items();
    let mut facts = Vec::new();
    for (template, stack) in held {
        let found = catalog
            .get(template)
            .ok_or(format!("no item template {template}"))?;
        facts.push((*stack, found));
        creature.inventory.push(sim::inventory::Held {
            template: template.to_string(),
            stack: *stack,
            slot: None,
            droppable: true,
            infinite: false,
        });
    }
    theirs.items = sim::inventory::usable(theirs.race, facts, |row| library.impact(row).is_some());
    creature.sheet = Some(theirs);
    let given = stage.level.clone().with_module(data);
    // Its scenes start from the install's scripts' state: the items
    // stocked.
    let given = crate::scenes::one_seat(given, None).map(|mut world| {
        let script = world.script_state().clone();
        Stage::of(&mut world).with_script(script)
    })?;
    Ok((given, theirs))
}

/// The usable kinds of the entity at `at` in `snapshot`, with what is left
/// of each.
pub(crate) fn kinds_of(snapshot: &Snapshot, at: usize) -> Vec<(u16, u8)> {
    let used = fighter(snapshot, at).pack.used;
    snapshot.entities[at]
        .sheet
        .map_or([None; KINDS], |sheet| sheet.items)
        .iter()
        .zip(used)
        .filter_map(|(kind, used)| kind.map(|kind| (kind.row, kind.count.saturating_sub(used))))
        .collect()
}

/// `kjk combat items`.
pub fn items(discovery: &Discovery, game: &str, module: &str, target_tag: &str) -> ExitCode {
    let mut report = Report::default();
    outln!("combat items {module}: KOTOR's consumables in a fight");

    outln!("  the rule");
    // Every row the native rules name is the row the game's table labels so
    // (a TSL row past K1's table is TSL's alone).
    match spell_labels(discovery, game, module) {
        Ok(labels) => {
            let wrong: Vec<String> = sim::combat::spells::NAMED
                .iter()
                .filter(|(name, row)| {
                    labels
                        .get(usize::from(*row))
                        .is_some_and(|label| label != name)
                })
                .map(|(name, row)| format!("{row} {name}: {:?}", labels.get(usize::from(*row))))
                .collect();
            let found = sim::combat::spells::NAMED
                .iter()
                .filter(|(_, row)| usize::from(*row) < labels.len())
                .count();
            report.check(
                wrong.is_empty() && found > 0,
                format!(
                    "the rows the native rules name: {found} of {} in {game}'s spells.2da, each labelled so{}",
                    sim::combat::spells::NAMED.len(),
                    if wrong.is_empty() {
                        String::new()
                    } else {
                        format!("; not: {}", wrong.join(", "))
                    }
                ),
            );
        }
        Err(error) => report.check(false, format!("spells.2da: {error}")),
    }
    report.check(
        items::use_of(ITEM_ABILITY_MED_PACK) == Some(Use::Heal { points: 10, per_rank: 1, droid: false })
            && items::use_of(ITEM_ABILITY_DROID_CONSTRUCTION_KIT) == Some(Use::Heal { points: 35, per_rank: 3, droid: true })
            && items::use_of(ITEM_ABILITY_ENERGY_SHIELD).is_some_and(|u| matches!(u, Use::Shield { points: 20, .. }))
            && items::use_of(ITEM_ABILITY_COMBAT_SHOT_3) == Some(Use::Stim(Stim::Battle { bonus: 3, vitality: 25 }))
            && items::use_of(ITEM_ABILITY_GRENADE_FRAGMENTATION) == Some(Use::Grenade)
            && items::use_of(FORCE_POWER_FORCE_PUSH).is_none(),
        "the table: medpac 10 + 1/rank, construction kit 35 + 3/rank, energy shield 20, Echani battle stim +3 / 25, grenades, other rows none".to_string(),
    );
    let mut medic = Sheet::dev_soldier();
    medic.skills[sim::combat::sheet::skills::TREAT_INJURY] = 4;
    report.check(
        items::healing(&medic, 20, 2, false) == 20 + 1 + 8
            && items::heal_ms(0) == 1500
            && items::heal_ms(4) == 1100
            && items::heal_ms(20) == 500,
        "an advanced medpac with WIS 12 and Treat Injury 4 heals 29; a use takes 1.5 s less 100 ms a rank, at least 0.5 s".to_string(),
    );

    let strike = sim::combat::damage::assess(
        &sim::combat::damage::Blow {
            attacker: None,
            relation: sim::combat::damage::Relation::Hostile,
            source: sim::combat::damage::Source::Power {
                points: 2,
                extra: 0,
                area: false,
                pushes: true,
                damage_type: Some(sim::combat::sheet::type_rows::ELECTRICAL),
            },
            location: sim::combat::Location::Chest,
            direction: None,
            sneak: 0,
        },
        None,
        &Sheet::dev_soldier(),
        sim::combat::FriendlyFire::Off,
        sim::combat::passive::Game::K1,
        &mut sim::SimRng::new(1, 7),
    );
    let mut shielded = Pack {
        shield: Some(items::Shielded {
            row: ITEM_ABILITY_ENERGY_SHIELD,
            types: match items::use_of(ITEM_ABILITY_ENERGY_SHIELD) {
                Some(items::Use::Shield { types, .. }) => types,
                _ => 0,
            },
            left: 20,
            until: u64::MAX,
        }),
        ..Pack::default()
    };
    let taken = items::absorb(
        &mut shielded,
        strike.damage_type.map(usize::from),
        strike.damage,
    );
    report.check(
        taken == strike.damage && shielded.shield.is_some_and(|shield| shield.left == 19),
        format!(
            "an energy shield takes a lightning strike (electrical): {:.2} HP absorbed, 19 points left",
            f64::from(taken) / 100.0
        ),
    );

    outln!("  in the arena");
    // A medpac at 0, again at 2 s, a third press at 4 s with two carried.
    let (stage, start) = arena(carrying(&[(ITEM_ABILITY_MED_PACK, 2)]), 1, 10_000);
    let me = seat0(&start);
    let run = stage.drive(
        &start,
        5000,
        &mut presses(me, buttons::MEDPAC, &[0, 2000, 4000]),
    );
    let heal = f64::from(items::healing(&carrying(&[]), 10, 1, false));
    report.check(
        hp_at(&run, me, 1400) == 100.0
            && hp_at(&run, me, 1600) == 100.0 + heal
            && hp_at(&run, me, 4900) == 100.0 + 2.0 * heal
            && fighter(run.snapshots.last().expect("stepped"), me).pack.used[0] == 2,
        format!(
            "medpac: 100 HP until it lands at 1.5 s, then {:.0}; a second at 2 s to {:.0}; the third press finds none",
            hp_at(&run, me, 1600),
            hp_at(&run, me, 4900)
        ),
    );
    report.replayed("medpacs", run.replayed);

    let mut droid = carrying(&[(ITEM_ABILITY_MED_PACK, 1), (ITEM_ABILITY_DROID_REPAIR_KIT, 1)]);
    droid.race = races::DROID;
    let (stage, start) = arena(droid, 1, 10_000);
    let run = stage.drive(&start, 2000, &mut presses(me, buttons::MEDPAC, &[0]));
    let used = fighter(run.snapshots.last().expect("stepped"), me)
        .pack
        .used;
    report.check(
        used == [0, 1, 0, 0] && hp_at(&run, me, 1900) == 100.0 + 15.0,
        format!(
            "a droid takes its repair kit, never the medpac: used {used:?}, {:.0} HP",
            hp_at(&run, me, 1900)
        ),
    );
    report.replayed("repair kit", run.replayed);

    let (stage, mut start) = arena(carrying(&[(ITEM_ABILITY_REMOVE_POISON, 1)]), 1, 10_000);
    if let Some(fighter) = start.entities[me].fighter.as_mut() {
        fighter.pack.poison = Some(Poisoned::new(
                sim::SimId(0),
                items::Course {
                    points: 4,
                    every_ms: 3000,
                    for_ms: 30_000,
                    ..items::Course::default()
                },
                start.time_ms,
                None,
        ));
        fighter.pack.drained = (items::drain(Ability::Dexterity, 6), start.time_ms + 30_000);
    }
    let run = stage.drive(&start, 500, &mut presses(me, buttons::MEDPAC, &[0]));
    let cured = fighter(run.snapshots.last().expect("stepped"), me).pack;
    report.check(
        !cured.afflicted(run.snapshots.last().expect("stepped").time_ms),
        "an antidote ends poison and weakness".to_string(),
    );
    report.replayed("antidote", run.replayed);

    let (stage, start) = arena(carrying(&[(ITEM_ABILITY_COMBAT_SHOT_3, 1), (ITEM_ABILITY_ADRENAL_SHOT_STRENGTH, 1)]), 1, 10_000);
    let run = stage.drive(&start, 1000, &mut presses(me, buttons::STIM, &[0, 500]));
    let after = fighter(run.snapshots.last().expect("stepped"), me);
    let affected = items::affected(
        carrying(&[]),
        &after.pack,
        run.snapshots.last().expect("stepped").time_ms,
    );
    let mut ended = after;
    items::settle(&mut ended, &carrying(&[]), u64::MAX, true, |_| false);
    // Its vitality soaks 30 points of damage: 25 from it, 5 from HP.
    let mut struck = after.pack;
    let soaked = struck.soak(3000);
    report.check(
        hp_at(&run, me, 100) == 100.0
            && after.pack.vitality == 2500
            && soaked == 2500
            && affected.attack == 3
            && items::stim_damage(&after.pack) == 3
            && after.pack.used == [1, 0, 0, 0]
            && ended.hp == 10_000
            && ended.pack.vitality == 0
            && ended.pack.stim.is_none(),
        "an Echani battle stim: 25 vitality taken before HP (30 damage: 25 from it), +3 to hit and damage; the adrenal waits (one stim at a time); its end takes what vitality is left, never HP".to_string(),
    );
    let strong = items::affected(
        carrying(&[]),
        &Pack {
            stim: Some(items::Stimmed {
                row: ITEM_ABILITY_HYPER_ADRENAL_SHOT_STRENGTH,
                until: u64::MAX,
            }),
            ..Pack::default()
        },
        0,
    );
    report.check(
        strong.abilities[Ability::Strength as usize] == Sheet::dev_soldier().abilities[0] + 6,
        "a hyper-adrenal: STR +6".to_string(),
    );
    report.replayed("stims", run.replayed);

    let (stage, start) = arena(carrying(&[(ITEM_ABILITY_RECOVERY_STIM, 1)]), 2, 10_000);
    let run = stage.drive(&start, 500, &mut presses(me, buttons::MEDPAC, &[0]));
    let last = run.snapshots.last().expect("stepped");
    let healed: Vec<i32> = last
        .entities
        .iter()
        .filter_map(|entity| entity.fighter.map(|fighter| fighter.hp))
        .collect();
    report.check(
        healed == vec![13_000, 13_000],
        format!(
            "a squad stim: both seats 100 → {:?} HP",
            healed.iter().map(|hp| hp / 100).collect::<Vec<_>>()
        ),
    );
    report.replayed("squad stim", run.replayed);

    outln!("  in {module}, against {target_tag}");
    let module_stage = match Stage::new(discovery, game, module, &[target_tag.to_string()]) {
        Ok(stage) => stage,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(target) = module_stage.creature(target_tag) else {
        eprintln!("no creature tagged {target_tag} in {module}");
        return ExitCode::FAILURE;
    };
    let me = seat0(&module_stage.start);
    let mut start = [320.0, 240.0, 160.0]
        .into_iter()
        .find_map(|distance| module_stage.scene(target, distance, carrying(&[(ITEM_ABILITY_ENERGY_SHIELD, 1)])))
        .expect("a clear spot near the target");
    if let Some(theirs) = start.entities[target].fighter.as_mut() {
        theirs.hp = 10_000_000;
    }
    let run = module_stage.run(&start, target, 15_000, &mut |ms| {
        (if ms == 0 { buttons::SHIELD } else { 0 }, 0, 0)
    });
    let up = run
        .snapshots
        .iter()
        .find(|snapshot| fighter(snapshot, me).pack.shield.is_some());
    let spent = run.snapshots.iter().find(|snapshot| {
        fighter(snapshot, me).pack.shield.is_none() && fighter(snapshot, me).pack.used[0] == 1
    });
    let full = fighter(&start, me).hp;
    let hp_while_up = run
        .snapshots
        .iter()
        .filter(|snapshot| fighter(snapshot, me).pack.shield.is_some())
        .map(|snapshot| fighter(snapshot, me).hp)
        .min();
    let end = fighter(run.snapshots.last().expect("stepped"), me);
    report.check(
        up.is_some() && hp_while_up == Some(full) && (spent.is_none() || end.hp <= full),
        format!(
            "an energy shield: up, no HP lost while it held ({}), {}",
            hp_while_up.map_or(0, |hp| hp / 100),
            match spent {
                Some(at) => format!(
                    "spent at {} ms; {:.2} HP after",
                    at.time_ms - start.time_ms,
                    f64::from(end.hp) / 100.0
                ),
                None => format!(
                    "{} points left at 15 s",
                    end.pack.shield.map_or(0, |s| s.left)
                ),
            }
        ),
    );
    report.replayed("shield", run.replayed);

    // A creature with a medpac in its inventory, hurt below a third of its
    // HP: its own AI scripts (`k_ai_master`) find the item's talent and use
    // it, its impact script (`k_sup_healing`) healing it.
    let medpac = [("g_i_medeqpmnt01", 1)];
    match given_items(&module_stage, target, &medpac, |_| ()) {
        Ok((given, theirs)) => {
            let mut start = given
                .scene(target, 160.0, carrying(&[]))
                .expect("a clear spot near the target");
            let low = i32::from(theirs.max_hp) * 30;
            if let Some(fighter) = start.entities[target].fighter.as_mut() {
                *fighter = Fighter {
                    hp: low,
                    ..*fighter
                };
                fighter.mind.cast_after = 0;
            }
            let run = given.run(&start, target, 5000, &mut |_| (0, 0, 0));
            let last = run.snapshots.last().expect("stepped");
            let hp = fighter(last, target).hp;
            // The medpacs its kinds show (the goods: the one given, any its
            // spawn script made) as the scene starts, and at its end.
            let medpacs = |snapshot: &Snapshot| -> u32 {
                kinds_of(snapshot, target)
                    .iter()
                    .filter(|(row, _)| *row == ITEM_ABILITY_MED_PACK)
                    .map(|(_, left)| u32::from(*left))
                    .sum()
            };
            let had = run.snapshots.first().map_or(0, medpacs);
            let left = medpacs(last);
            report.check(
                hp > low && had >= 1 && left + 1 == had,
                format!(
                    "{target_tag} at 30% used a medpac it carries by its own scripts ({:.2} → {:.2} HP; medpacs {had} → {left})",
                    f64::from(low) / 100.0,
                    f64::from(hp) / 100.0
                ),
            );
            report.replayed("a creature's medpac", run.replayed);
        }
        Err(error) => report.check(false, error),
    }

    report.finish(serde_json::json!({
        "verb": "combat items", "game": game, "module": module, "target": target_tag,
    }))
}
