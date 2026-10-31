//! The party's weapons, sales, goods and what the player wears.

use super::*;

/// Wearing the strongest weapon of the other kind (`ranged` or not) the
/// party carries in the player's right hand (by its damage's average, as
/// its upgrades leave it), if it carries one: back from a gun, the saber,
/// not the first blade in the pack.
pub(super) fn other_weapon(world: &sim::SimWorld, snapshot: &Snapshot, ranged: bool) -> Option<Action> {
    let catalog = world.level().scripts()?.items();
    let hand = sim::inventory::slots::RIGHT_WEAPON;
    let feats = crate::cast::me(snapshot, Seat(0))
        .and_then(|me| snapshot.get(me)?.sheet)
        .map(|sheet| sheet.feats);
    let candidates: Vec<(u32, sim::combat::sheet::Weapon, Vec<u16>, u32)> = snapshot
        .goods
        .carried(sim::inventory::PARTY)
        .filter_map(|(id, item)| {
            let facts = catalog.get(&item.template)?;
            let weapon = facts.fitted(item.upgrades, &item.fitted, catalog).weapon?;
            Some((id, weapon, facts.needs.clone(), facts.slots))
        })
        .collect();
    strongest(&candidates, feats.as_ref(), ranged).map(|item| Action::Equip { item, slot: hand })
}

/// Of `candidates` (an item, the weapon it is, the feats it needs and the
/// slots it goes in), the strongest for the right hand by twice its average
/// damage, the first on a tie, of the kind other than `ranged` (a blade
/// when true, a gun when false); only one `feats` is trained for (the
/// equip screen refuses the rest: Mira has no melee proficiency), all
/// without a sheet.
pub(super) fn strongest(
    candidates: &[(u32, sim::combat::sheet::Weapon, Vec<u16>, u32)],
    feats: Option<&sim::combat::sheet::Rows<4>>,
    ranged: bool,
) -> Option<u32> {
    let hand = sim::inventory::slots::RIGHT_WEAPON;
    candidates
        .iter()
        .filter(|(_, weapon, needs, slots)| {
            feats.is_none_or(|feats| sim::combat::passive::may_equip(feats, needs))
                && weapon.delivery.ranged() != ranged
                && slots & 1 << hand != 0
        })
        .map(|(id, weapon, ..)| {
            let twice_average = i32::from(weapon.dice) * (i32::from(weapon.die) + 1)
                + i32::from(weapon.bonus_dice) * (i32::from(weapon.bonus_die) + 1)
                + 2 * i32::from(weapon.damage_bonus);
            (twice_average, *id)
        })
        .max_by_key(|(strength, id)| (*strength, std::cmp::Reverse(*id)))
        .map(|(_, item)| item)
}

/// The template a route's `equip` names: `saber`, the strongest lightsaber
/// the party carries or the player wears (by its damage's average, as its
/// upgrades leave it: the treasure scripts' saber differs with the level
/// the player has when they make it); any other word, that template.
pub(super) fn named_item(world: &sim::SimWorld, snapshot: &Snapshot, named: &str) -> Option<String> {
    if named != "saber" {
        return Some(named.to_string());
    }
    let catalog = world.level().scripts()?.items();
    let me = crate::cast::me(snapshot, Seat(0))?;
    world
        .script_state()
        .items
        .values()
        .filter(|item| item.holder == sim::inventory::PARTY || item.holder == me.0)
        .filter_map(|item| {
            let facts = catalog.get(&item.template)?;
            let weapon = facts.fitted(item.upgrades, &item.fitted, catalog).weapon?;
            matches!(weapon.delivery, sim::combat::sheet::Delivery::Saber(_)).then_some(())?;
            let twice_average = i32::from(weapon.dice) * (i32::from(weapon.die) + 1)
                + i32::from(weapon.bonus_dice) * (i32::from(weapon.bonus_die) + 1)
                + 2 * i32::from(weapon.damage_bonus);
            Some((twice_average, item.template.clone()))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
        .map(|(_, template)| template)
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

/// How many of `template` the party carries.
pub(super) fn carried(snapshot: &Snapshot, template: &str) -> u32 {
    snapshot
        .goods
        .carried(sim::inventory::PARTY)
        .filter(|(_, item)| item.template.eq_ignore_ascii_case(template))
        .map(|(_, item)| u32::from(item.stack.max(1)))
        .sum()
}

/// One of `template` bought from the store open to the player, as its
/// screen's buy button sends it: the first of its stock with that
/// template; why not, with none open or none in its stock.
pub(super) fn purchase(snapshot: &Snapshot, template: &str) -> Result<Option<Action>, String> {
    let Some(me) = crate::cast::me(snapshot, Seat(0)) else {
        return Ok(None);
    };
    let Some((_, trade)) = snapshot.goods.trades.iter().find(|(who, _)| *who == me.0) else {
        return Ok(None);
    };
    snapshot
        .goods
        .carried(trade.store)
        .find(|(_, item)| item.template.eq_ignore_ascii_case(template))
        .map(|(item, _)| {
            Some(Action::Buy {
                store: trade.store,
                item,
            })
        })
        .ok_or_else(|| {
            let held: Vec<&str> = snapshot
                .goods
                .carried(trade.store)
                .map(|(_, item)| item.template.as_str())
                .collect();
            format!("buy: the store holds no {template} (it holds {held:?})")
        })
}

/// One of `template` made at the bench `bench` shows (a workbench, or the
/// lab station), as its creation screen's Create button sends it: the
/// first row of its table that makes it; why not, with none listing it.
pub(super) fn creation(world: &sim::SimWorld, bench: &sim::notice::Bench, template: &str) -> Result<Action, String> {
    let table = world
        .level()
        .scripts()
        .and_then(|library| library.workshop().table(&bench.list))
        .ok_or(format!("craft: no table {}", bench.list))?;
    let row = table
        .iter()
        .position(|recipe| recipe.template.eq_ignore_ascii_case(template))
        .ok_or(format!("craft: {} does not make {template}", bench.list))?;
    Ok(Action::Create {
        bench: sim::action::Workbench {
            list: bench.list.clone(),
            lab: bench.lab,
            maker: bench.maker,
        },
        row: u16::try_from(row).map_err(|_| format!("craft: row {row}"))?,
    })
}

/// The item a route's `fit` names (`named_item`'s word), the player's worn
/// one before one the party carries, and whether `upgrade` is fitted in it
/// yet.
pub(super) fn fitted(world: &sim::SimWorld, snapshot: &Snapshot, named: &str, upgrade: &str) -> Option<(u32, bool)> {
    let me = crate::cast::me(snapshot, Seat(0))?;
    let named = named_item(world, snapshot, named)?;
    let items = &world.script_state().items;
    items
        .iter()
        .filter(|(_, item)| item.template.eq_ignore_ascii_case(&named))
        .filter(|(_, item)| item.holder == me.0 || item.holder == sim::inventory::PARTY && item.slot.is_none())
        .min_by_key(|(id, item)| (item.holder != me.0, **id))
        .map(|(id, item)| {
            let fitted = item.fitted.iter().any(|part| part.eq_ignore_ascii_case(upgrade));
            (*id, fitted)
        })
}

/// `upgrade` (a template the party carries) fitted into `item`, as the
/// upgrade screen's Assemble sends one change; why not, with none carried.
pub(super) fn fitting(world: &sim::SimWorld, item: u32, upgrade: &str) -> Result<Action, String> {
    world
        .script_state()
        .items
        .iter()
        .find(|(_, part)| {
            part.holder == sim::inventory::PARTY && part.slot.is_none() && part.template.eq_ignore_ascii_case(upgrade)
        })
        .map(|(part, _)| Action::Fit {
            item,
            upgrade: *part,
            replace: None,
        })
        .ok_or(format!("fit: the party carries no {upgrade}"))
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
        .filter_map(|item| {
            let fitted: Vec<&str> = item.fitted.iter().filter(|part| !part.is_empty()).map(String::as_str).collect();
            let fitted = if fitted.is_empty() { String::new() } else { format!(" [{}]", fitted.join(", ")) };
            item.slot.map(|slot| format!("{slot}: {}{fitted}", item.template))
        })
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

/// What the equip screen would send to wear what the party carries: each
/// item in the first slot it fits that the player does not wear yet; or,
/// `named`, that item in the first slot it fits, worn or not.
/// Whether the player wears an item of template `named`.
pub(super) fn wears(world: &sim::SimWorld, snapshot: &Snapshot, named: &str) -> bool {
    crate::cast::me(snapshot, Seat(0)).is_some_and(|me| {
        world.script_state().items.values().any(|item| {
            item.holder == me.0 && item.slot.is_some() && item.template.eq_ignore_ascii_case(named)
        })
    })
}

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
        .filter(|(_, item)| named.is_none_or(|named| item.template.eq_ignore_ascii_case(named)))
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

/// The player's body (its script object id), and whether an effect of
/// power `row` lasts on it (its cast has landed, `Lasting::spell`).
pub(super) fn under(snapshot: &Snapshot, row: u16) -> Option<(u32, bool)> {
    let me = crate::cast::me(snapshot, Seat(0))?;
    let lasts = snapshot.get(me).and_then(|entity| entity.fighter).is_some_and(|fighter| {
        fighter
            .effects
            .lasting
            .iter()
            .flatten()
            .any(|lasting| lasting.spell == row && lasting.until > snapshot.time_ms)
    });
    Some((me.0, lasts))
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
