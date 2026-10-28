//! `kjk sim probes "ute encounters in the games"`: the games' own UTE
//! encounters as a plan reads them (`session::objects::encounters`), where
//! this machine has the installs (each check passes, saying so, where it
//! has not): K1's Vulkar patrol in `tar_m03ab` (its fields, its creatures
//! by challenge, `encdifficulty.2da`'s value for its row, its outline read
//! from the GIT's `X`/`Y`/`Z` points), TSL's mercenaries in `204tel` and
//! their list, and `506ond`'s palace battle (26 encounters, 12 active, the
//! dark side's 14 waiting for `k_506_enter`); every creature an encounter names is one the module's
//! script library can make.

use sim::module::{ModuleData, ObjectData, ObjectKind};
use sim::probe::{Checks, check};
use sim::spawner::Encounter;

/// A module of `game` as a plan reads it, with its script library; `None`
/// where this machine lacks the install.
fn planned(game: &str, module: &str) -> Option<Result<sim::Level, String>> {
    let installer = crate::scenes::installer(&crate::discover(), game).ok()?;
    let request = installer.request(module, None, 1);
    Some(installer.plan_without_encounters(&request).map(|plan| plan.level))
}

/// The encounters of `data` (object, UTE), in the module's order.
fn encounters(data: &ModuleData) -> Vec<(ObjectData, Encounter)> {
    data.objects()
        .filter(|object| object.kind == ObjectKind::Encounter)
        .filter_map(|object| {
            let ute = (**object.encounter.as_ref()?).clone();
            Some((object.into_owned(), ute))
        })
        .collect()
}

/// Every creature `level`'s encounters name that its library cannot make.
fn unmade(level: &sim::Level) -> Vec<String> {
    let (Some(data), Some(library)) = (level.module(), level.scripts()) else {
        return vec!["no module or library".into()];
    };
    encounters(data)
        .iter()
        .flat_map(|(_, ute)| ute.creatures.iter())
        .filter(|creature| library.creature(&creature.template).is_none())
        .map(|creature| creature.template.clone())
        .collect()
}

/// Runs `test` on `module` of `game`, or passes without the install.
fn with_module(
    game: &str,
    module: &str,
    test: impl FnOnce(&sim::Level) -> Result<String, String>,
) -> Result<String, String> {
    match planned(game, module) {
        None => Ok(format!("no {game} install: not checked")),
        Some(Err(error)) => Err(error),
        Some(Ok(level)) => test(&level),
    }
}

fn vulkar_patrol() -> Result<String, String> {
    with_module("k1", "tar_m03ab", |level| {
        let data = level.module().ok_or("no module")?;
        let found = encounters(data);
        let (object, ute) = found
            .iter()
            .find(|(object, _)| object.tag == "ptar_vulkaraptptl")
            .ok_or("no ptar_vulkaraptptl")?;
        let order: Vec<&str> = ute.creatures.iter().map(|c| c.template.as_str()).collect();
        let outline = object.geometry.len() == 5 && object.geometry.iter().any(|p| p[0] != 0.0);
        let missing = unmade(level);
        check(
            ute.max_creatures == 3
                && ute.rec_creatures == 2
                && ute.difficulty == 5
                && ute.reset
                && ute.reset_time_s == 30
                && ute.respawns == -1
                && ute.player_only
                && object.faction == 1
                && order == ["tar03_blackv042", "tar03_blackv040", "tar03_blackv041"]
                && ute.points.len() == 1
                && outline
                && missing.is_empty(),
            format!(
                "{} encounters; patrol {order:?}, difficulty {}, reset {} s × {}, outline {:?}; unmade {missing:?}",
                found.len(),
                ute.difficulty,
                ute.reset_time_s,
                ute.respawns,
                object.geometry
            ),
        )
    })
}

fn mercenaries() -> Result<String, String> {
    with_module("tsl", "204tel", |level| {
        let data = level.module().ok_or("no module")?;
        let found = encounters(data);
        let (_, ute) = found
            .iter()
            .find(|(object, _)| object.tag == "merc_entry")
            .ok_or("no merc_entry")?;
        let list: Vec<String> = sim::spawner::tsl_list(ute, &mut sim::SimRng::new(1, 0))
            .into_iter()
            .map(|index| ute.creatures[index].template.clone())
            .collect();
        let missing = unmade(level);
        check(
            found.len() == 3
                && found.iter().all(|(_, ute)| !ute.active)
                && ute.points.len() == 6
                && list.len() == 6
                && list[0] == "203_merc004"
                && missing.is_empty(),
            format!("{} encounters, inactive; merc_entry's list {list:?}; unmade {missing:?}", found.len()),
        )
    })
}

fn palace_battle() -> Result<String, String> {
    with_module("tsl", "506ond", |level| {
        let data = level.module().ok_or("no module")?;
        let found = encounters(data);
        let (active, waiting): (Vec<_>, Vec<_>) = found.iter().partition(|(_, ute)| ute.active);
        let dark = waiting.iter().all(|(object, _)| object.tag.starts_with("ds_"));
        let missing = unmade(level);
        check(
            found.len() == 26 && active.len() == 12 && dark && missing.is_empty(),
            format!(
                "{} encounters, {} active, the {} waiting all the dark side's {dark}; unmade {missing:?}",
                found.len(),
                active.len(),
                waiting.len()
            ),
        )
    })
}

pub fn probes() -> Checks {
    vec![
        ("k1_tar_m03ab_vulkar_patrol", vulkar_patrol()),
        ("tsl_204tel_mercenaries", mercenaries()),
        ("tsl_506ond_palace_battle", palace_battle()),
    ]
}
