//! The game's own names, icons and search (Task 3, Review Focus 4).

use crate::model::catalog::{Catalog, Entry};
use crate::model::locate;

fn entry(key: u32, resref: &str, name: &str) -> Entry {
    Entry::new(key, resref, name)
}

/// Every word must match, any case, by name or by resref; ten thousand
/// entries are searched in under 5 ms.
pub fn search() -> Result<String, String> {
    let list = [
        entry(0, "g_w_lghtsbr01", "Lightsaber"),
        entry(1, "g_w_dblsbr001", "Double-bladed Lightsaber"),
        entry(2, "g_w_blstrpstl001", "Blaster Pistol"),
    ];
    let found = |query: &str| -> Vec<u32> {
        Catalog::search(&list, query)
            .map(|entry| entry.key)
            .collect()
    };
    let cases: [(&str, &[u32]); 5] = [
        ("saber", &[0, 1]),
        ("double saber", &[1]),
        ("g_w_lghtsbr01", &[0]),
        ("SABER", &[0, 1]),
        ("", &[0, 1, 2]),
    ];
    for (query, want) in cases {
        if found(query) != want {
            return Err(format!("{query:?} found {:?}, not {want:?}", found(query)));
        }
    }
    let many: Vec<Entry> = (0..10_000)
        .map(|n| {
            entry(
                n,
                &format!("g_i_item{n:05}"),
                &format!("Item number {n} of many"),
            )
        })
        .collect();
    let start = std::time::Instant::now();
    let hits = Catalog::search(&many, "number 9999").count();
    let took = start.elapsed();
    if hits != 1 || took.as_millis() >= 5 {
        return Err(format!("10 000 entries: {hits} hits in {took:?}"));
    }
    Ok(format!("5 queries as wanted; 10 000 entries in {took:?}"))
}

/// `game`'s install read: the lightsaber by its own name with an icon that
/// decodes, Force Lightning among the powers with one, quests, and (TSL)
/// items K1 has not.
pub fn install(game: vfs::Game) -> Result<String, String> {
    let discovery = locate::discovery();
    if discovery.get(game).is_none() {
        return Ok(format!("skipped: no {} install", game.key()));
    }
    let catalog = Catalog::load(&discovery, game)?;
    let saber = catalog
        .items
        .iter()
        .find(|entry| entry.resref == "g_w_lghtsbr01")
        .ok_or("no g_w_lghtsbr01")?;
    if saber.name != "Lightsaber" {
        return Err(format!("the lightsaber is called {:?}", saber.name));
    }
    let icon = |name: &Option<String>| {
        name.as_deref()
            .and_then(|name| catalog.icon(name))
            .filter(|icon| icon.size[0] > 0 && icon.rgba.len() == icon.size[0] * icon.size[1] * 4)
    };
    let saber_icon = icon(&saber.icon).ok_or(format!(
        "the lightsaber's icon {:?} does not decode",
        saber.icon
    ))?;
    let lightning = catalog
        .powers
        .iter()
        .find(|entry| entry.name == "Force Lightning")
        .ok_or("no Force Lightning among the powers")?;
    icon(&lightning.icon).ok_or(format!(
        "Force Lightning's icon {:?} does not decode",
        lightning.icon
    ))?;
    if catalog.quests.is_empty() || catalog.feats.is_empty() || catalog.skills.is_empty() {
        return Err(format!(
            "{} quests, {} feats, {} skills",
            catalog.quests.len(),
            catalog.feats.len(),
            catalog.skills.len()
        ));
    }
    let named = catalog.items.iter().filter(|entry| entry.named()).count();
    if named * 10 < catalog.items.len() * 9 {
        return Err(format!("{named} of {} items named", catalog.items.len()));
    }
    Ok(format!(
        "{} items ({named} named), {} feats, {} powers, {} quests; the saber's icon {:?} {}×{}",
        catalog.items.len(),
        catalog.feats.len(),
        catalog.powers.len(),
        catalog.quests.len(),
        saber.icon,
        saber_icon.size[0],
        saber_icon.size[1]
    ))
}

/// TSL holds items K1 has not, by name (spec: both games).
pub fn tsl_own_items() -> Result<String, String> {
    let discovery = locate::discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() || discovery.get(vfs::Game::Kotor2).is_none() {
        return Ok("skipped: needs both installs".into());
    }
    let k1 = Catalog::load(&discovery, vfs::Game::Kotor1)?;
    let tsl = Catalog::load(&discovery, vfs::Game::Kotor2)?;
    let own: Vec<&Entry> = tsl
        .items
        .iter()
        .filter(|entry| entry.named() && !k1.items.iter().any(|k1| k1.resref == entry.resref))
        .collect();
    match own.first() {
        Some(first) if own.len() > 50 => Ok(format!(
            "{} TSL-only items, as {} ({})",
            own.len(),
            first.name,
            first.resref
        )),
        _ => Err(format!("{} TSL-only items", own.len())),
    }
}

/// With K1, a companion's template gives her name and a portrait that
/// decodes (the party strip).
pub fn creature_k1() -> Result<String, String> {
    let discovery = locate::discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() {
        return Ok("skipped: no k1 install".into());
    }
    let catalog = Catalog::load(&discovery, vfs::Game::Kotor1)?;
    let bastila = catalog.creature("p_bastilla");
    let portrait = bastila
        .portrait
        .as_deref()
        .and_then(|name| catalog.icon(name))
        .ok_or(format!(
            "her portrait {:?} does not decode",
            bastila.portrait
        ))?;
    if bastila.name != "Bastila" {
        return Err(format!("p_bastilla is called {:?}", bastila.name));
    }
    // A companion whose template the install lacks, by the template the
    // party table keeps in the save (`Npc::template_data`).
    let kept = catalog
        .data
        .source
        .0
        .find("p_bastilla", fmt_odyssey::ResType::UTC)
        .and_then(|found| found.location.read().ok())
        .ok_or("no p_bastilla.utc")?;
    let from_save = catalog.creature_from(&kept);
    if from_save != bastila {
        return Err(format!("from the kept template: {from_save:?}"));
    }
    let unknown = catalog.creature("no_such_npc");
    if !unknown.name.is_empty() || unknown.portrait.is_some() {
        return Err(format!("a template the install lacks: {unknown:?}"));
    }
    Ok(format!(
        "{} ({:?}, {}×{})",
        bastila.name, bastila.portrait, portrait.size[0], portrait.size[1]
    ))
}
