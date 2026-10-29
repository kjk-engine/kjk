//! The party's weapons, sales, goods and what the player wears.

use super::*;

/// Wearing the strongest weapon of the other kind (`ranged` or not) the
/// party carries in the player's right hand (by its damage's average, as
/// its upgrades leave it), if it carries one: back from a gun, the saber,
/// not the first blade in the pack.
pub(super) fn other_weapon(world: &sim::SimWorld, snapshot: &Snapshot, ranged: bool) -> Option<Action> {
    let catalog = world.level().scripts()?.items();
    let hand = sim::inventory::slots::RIGHT_WEAPON;
    snapshot
        .goods
        .carried(sim::inventory::PARTY)
        .filter_map(|(id, item)| {
            let facts = catalog.get(&item.template)?;
            let weapon = facts
                .fitted(item.upgrades, &item.fitted, catalog)
                .weapon
                .filter(|weapon| weapon.delivery.ranged() != ranged)?;
            let twice_average = i32::from(weapon.dice) * (i32::from(weapon.die) + 1)
                + i32::from(weapon.bonus_dice) * (i32::from(weapon.bonus_die) + 1)
                + 2 * i32::from(weapon.damage_bonus);
            (facts.slots & 1 << hand != 0).then_some((twice_average, id))
        })
        .max_by_key(|(strength, id)| (*strength, std::cmp::Reverse(*id)))
        .map(|(_, item)| Action::Equip { item, slot: hand })
}

/// The dearest item the party carries that the store open to the player
/// buys, sold to it: never a plot item nor what a player keeps to use
/// (grenades, medpacs and the like; spikes, of the base items `kept`: the
/// Star Forge's computers want them); none without a store open.
pub(super) fn sale(world: &sim::SimWorld, snapshot: &Snapshot, kept: &[u16]) -> Option<Action> {
    let me = crate::cast::me(snapshot, Seat(0))?;
    let (_, trade) = snapshot.goods.trades.iter().find(|(who, _)| *who == me.0)?;
    let catalog = world.level().scripts()?.items();
    snapshot
        .goods
        .carried(sim::inventory::PARTY)
        .filter(|(_, item)| trade.terms.buys && !item.plot)
        .filter_map(|(id, item)| catalog.get(&item.template).map(|facts| (facts, id)))
        .filter(|(facts, _)| facts.combat_row().is_none() && !kept.contains(&facts.base_item))
        .map(|(facts, id)| (facts.cost, id))
        .max()
        .map(|(_, item)| Action::Sell {
            store: trade.store,
            item,
        })
}

/// What the party carries, what the player wears (slot: template) and how
/// hale the player and its companions are (HP of the most), at the end of
/// the run: a chain's party worn down route by route shows here.
pub(super) fn goods(world: &sim::SimWorld, snapshot: &Snapshot) -> serde_json::Value {
    let party = &world.script_state().party;
    let health: Vec<String> = crate::cast::me(snapshot, Seat(0))
        .into_iter()
        .chain(
            party
                .members
                .iter()
                .filter_map(|npc| party.available.get(npc)?.object)
                .map(sim::SimId),
        )
        .filter_map(|id| snapshot.get(id))
        .filter_map(|e| {
            let (fighter, sheet) = e.fighter.zip(e.sheet)?;
            Some(format!("{}: {}/{}", e.id.0, fighter.hp / 100, sheet.max_hp))
        })
        .collect();
    let me = crate::cast::me(snapshot, Seat(0)).map(|me| me.0);
    let items = &world.script_state().items;
    let carried: Vec<String> = items
        .values()
        .filter(|item| item.holder == sim::inventory::PARTY)
        .map(|item| format!("{} x{}", item.template, item.stack))
        .collect();
    let worn: Vec<String> = items
        .values()
        .filter(|item| Some(item.holder) == me)
        .filter_map(|item| item.slot.map(|slot| format!("{slot}: {}", item.template)))
        .collect();
    json!({
        "gold": world.script_state().gold,
        "carried": carried,
        "worn": worn,
        "health": health,
        "player": player(world, snapshot),
    })
}

/// The first seat's own character as its levels made it: its career
/// (class, levels), level, experience, scores, skill ranks, feats and
/// powers (by row).
fn player(world: &sim::SimWorld, snapshot: &Snapshot) -> serde_json::Value {
    use sim::combat::sheet::Rows;
    let Some((id, sheet)) = snapshot.own(Seat(0)).and_then(|own| Some((own.id, own.sheet?))) else {
        return serde_json::Value::Null;
    };
    let feats: Vec<u16> = (0..Rows::<4>::ROWS).filter(|feat| sheet.feats.has(*feat)).collect();
    let powers: Vec<u16> = (0..Rows::<5>::ROWS).filter(|power| sheet.powers.has(*power)).collect();
    json!({
        "career": world.script_state().careers.get(&id.0),
        "level": sheet.level,
        "xp": world.script_state().xp(sim::inventory::PARTY),
        "abilities": sheet.abilities,
        "skills": sheet.skills,
        "feats": feats,
        "powers": powers,
    })
}

/// Whether `template` is one `named` names: the template itself, or, with
/// `*` in it, any template the pattern matches (`*` any run of characters,
/// case aside), for an item the game picks at random, as the Exile's own
/// lightsaber is (`a_give_quest_ls` rolls its kind and its tier at the
/// player's level): `g_w_*sbr*` any lightsaber.
pub(super) fn names(named: &str, template: &str) -> bool {
    let (named, template) = (named.to_ascii_lowercase(), template.to_ascii_lowercase());
    let Some((first, tail)) = named.split_once('*') else {
        return named == template;
    };
    let Some(mut rest) = template.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = tail.split('*').collect();
    let Some((last, middle)) = parts.split_last() else {
        return false;
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// Whether the player wears an item of template `named` (`names`).
pub(super) fn wears(world: &sim::SimWorld, snapshot: &Snapshot, named: &str) -> bool {
    crate::cast::me(snapshot, Seat(0)).is_some_and(|me| {
        world.script_state().items.values().any(|item| {
            item.holder == me.0 && item.slot.is_some() && names(named, &item.template)
        })
    })
}

/// What the equip screen would send to wear what the party carries: each
/// item in the first slot it fits that the player does not wear yet; or,
/// `named` (`names`), the first such item made (the lowest id) in the first
/// slot it fits, worn or not.
pub(super) fn equip(world: &sim::SimWorld, snapshot: &Snapshot, named: Option<&str>) -> Vec<Action> {
    let Some(me) = crate::cast::me(snapshot, Seat(0)) else {
        return Vec::new();
    };
    let Some(library) = world.level().scripts() else {
        return Vec::new();
    };
    let race = snapshot.get(me).and_then(|e| e.sheet).map(|sheet| sheet.race);
    let items = &world.script_state().items;
    let mut worn: u32 = items
        .values()
        .filter(|item| item.holder == me.0)
        .filter_map(|item| item.slot)
        .fold(0, |worn, slot| worn | 1 << slot);
    let mut wear = Vec::new();
    for (id, item) in items
        .iter()
        .filter(|(_, item)| item.holder == sim::inventory::PARTY)
        .filter(|(_, item)| named.is_none_or(|named| names(named, &item.template)))
    {
        // Only what the player's race may wear (`ItemFacts::fits`), as the
        // equip screen lists.
        let slots = library
            .items()
            .get(&item.template)
            .filter(|facts| race.is_none_or(|race| facts.fits(race)))
            .map_or(0, |facts| facts.slots);
        if named.is_some() {
            if let Some(slot) = (0..32u8).find(|bit| slots & (1 << bit) != 0) {
                return vec![Action::Equip { item: *id, slot }];
            }
            continue;
        }
        if let Some(slot) = (0..32u8).find(|bit| slots & !worn & (1 << bit) != 0) {
            worn |= 1 << slot;
            wear.push(Action::Equip { item: *id, slot });
        }
    }
    wear
}

/// The armed, disarmable mine (a trap trigger) the party sees nearest `at`
/// (metres), within 3 m of it: its object index.
pub(super) fn mine_near(here: &Here, at: [f32; 2]) -> Option<u32> {
    use sim::trap;
    here.snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let sim::Kind::Object(ObjectKind::Trigger, index) = entity.kind else {
                return None;
            };
            let state = entity.object?;
            if state.trap & trap::PARTY_SEES == 0 || state.trap & trap::DISABLED != 0 {
                return None;
            }
            let facts = here.facts_of(entity)?;
            facts.trap.as_ref().filter(|trap| trap.disarmable)?;
            let centre = sim::traps::centre(&facts.geometry, entity.position);
            let d = (centre[0] - at[0]).hypot(centre[1] - at[1]);
            (d <= 3.0).then_some((d, index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| index)
}
