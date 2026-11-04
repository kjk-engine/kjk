//! The editor's checks (the approved test `save_edits`): each a name and
//! what it found, or why it failed.

use std::path::{Path, PathBuf};

type Probe = (&'static str, Result<String, String>);

/// Every check, in order.
pub fn all() -> Vec<Probe> {
    vec![
        ("folder_platform", folder_platform()),
        ("folder_no_home", folder_no_home()),
        ("saves_folder", saves_folder()),
        ("open_refusals", open_refusals()),
        ("open_from_folder", open_from_folder()),
        ("data_k1", data_of(vfs::Game::Kotor1)),
        ("data_tsl", data_of(vfs::Game::Kotor2)),
        ("characters_listed", characters_listed()),
        ("characters_unsheeted", characters_unsheeted()),
        ("edits_reach_the_world", edits_reach_the_world()),
        ("unknown_item_refused", unknown_item_refused()),
    ]
}

const MEDPAC: &str = "g_i_medeqpmnt01";
const PISTOL: &str = "g_w_blstrpstl001";

/// Two items scripts may make: a medpac stacking to ten, and a pistol.
fn catalog() -> sim::inventory::Catalog {
    let mut medpac = sim::inventory::ItemFacts::plain(MEDPAC);
    medpac.stacking = 10;
    sim::inventory::Catalog::new([medpac, sim::inventory::ItemFacts::plain(PISTOL)])
}

/// `save` round-tripped, loaded in `level` and stepped once: the world as
/// it then stands.
fn stepped(save: &sim::Save, level: &sim::Level) -> Result<sim::SimWorld, String> {
    let again = sim::Save::decode(&save.encode()).map_err(|e| e.to_string())?;
    if again != *save {
        return Err("the edited save does not round-trip".into());
    }
    let mut world = save.world(level, 2, None)?;
    let tick = save.snapshot.tick + 1;
    let input = sim::determinism::pilot(1, tick, 2);
    sim::step(&mut world, tick, &input, 1_000 / save.hz, sim::StepReason::Authority);
    Ok(world)
}

/// Credits, the party's XP, a quest, the three kinds of global and the
/// party's items edited; a step after loading keeps every one, in the
/// scripts' state and in the snapshot's views of it (the proving ground
/// has no scripts to gather the views again: the editor shows them as
/// set, and the party's items are the state's).
fn edits_reach_the_world() -> Result<String, String> {
    let (level, mut save) = sim::fixtures::kitchen_sink();
    let catalog = catalog();
    let party = sim::inventory::PARTY;
    let (medpacs, pistol) = {
        let mut edit = crate::Editor::new(&mut save, &catalog);
        edit.set_credits(999)?;
        edit.set_party_xp(4_000)?;
        edit.set_quest("k_fixture_quest", 30)?;
        edit.set_number("k_fixture_number", 11)?;
        edit.set_boolean("K_FIXTURE_BOOLEAN", false)?;
        edit.set_string("K_FIXTURE_STRING", "edited")?;
        let medpacs = edit.add_item(party, MEDPAC, 3)?;
        edit.set_stack(medpacs, 5)?;
        let pistol = edit.add_item(party, PISTOL, 1)?;
        edit.remove_item(pistol)?;
        (medpacs, pistol)
    };
    let mut world = stepped(&save, &level)?;
    let snapshot = world.snapshot();
    let state = world.script_state();
    let goods = &snapshot.goods;
    let held = |id: u32| state.items.get(&id);
    let checks = [
        ("credits", goods.gold == 999 && state.gold == 999),
        ("xp", snapshot.progress.xp == 4_000 && state.xp(party) == 4_000),
        ("quest", snapshot.progress.journal.contains(&("k_fixture_quest".into(), 30))),
        ("number", state.numbers.get("K_FIXTURE_NUMBER") == Some(&11)),
        ("boolean", state.booleans.get("K_FIXTURE_BOOLEAN") == Some(&false)),
        ("string", state.strings.get("K_FIXTURE_STRING").map(String::as_str) == Some("edited")),
        ("medpacs", held(medpacs).is_some_and(|item| item.stack == 5 && item.template == MEDPAC)),
        ("pistol gone", held(pistol).is_none()),
    ];
    let failed: Vec<&str> = checks.iter().filter(|(_, ok)| !ok).map(|(name, _)| *name).collect();
    if failed.is_empty() {
        Ok(format!("{} edits seen after a step", checks.len()))
    } else {
        Err(format!("not seen: {failed:?}"))
    }
}

/// An item no install holds is refused by name, and nothing changes.
fn unknown_item_refused() -> Result<String, String> {
    let (_, mut save) = sim::fixtures::kitchen_sink();
    let before = save.clone();
    let catalog = catalog();
    let refused = crate::Editor::new(&mut save, &catalog).add_item(sim::inventory::PARTY, "no_such_item", 1);
    match refused {
        Err(why) if why.contains("no_such_item") && save == before => Ok(why),
        Err(why) => Err(format!("refused ({why}) but the save changed")),
        Ok(id) => Err(format!("made {id}")),
    }
}

/// The kitchen sink's characters: its players by seat first, then the
/// companion the table holds, away, as its template stands.
fn characters_listed() -> Result<String, String> {
    use crate::Who;
    let (level, save) = sim::fixtures::kitchen_sink();
    let listed = crate::characters(&save, Some(&level));
    let players = save
        .snapshot
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, sim::Kind::Avatar(_)))
        .count();
    let want = players + save.script.party.available.len();
    if listed.len() != want {
        return Err(format!("{} listed, not {want}", listed.len()));
    }
    if !matches!(listed[0].who, Who::Player(sim::Seat(0))) {
        return Err(format!("first {:?}", listed[0].who));
    }
    let away = listed
        .iter()
        .find(|character| character.who == Who::Away { npc: 0 })
        .ok_or("the table's companion is not listed")?;
    if away.template != "p_fixture" || away.hp != i32::from(away.sheet.max_hp) * 100 {
        return Err(format!("away as {} at {} hp", away.template, away.hp));
    }
    Ok(format!("{players} players, {} away", listed.len() - players))
}

/// A member whose body has no sheet of its own and whose module has no
/// facts for it is told apart, not listed as a commoner.
fn characters_unsheeted() -> Result<String, String> {
    let (level, mut save) = sim::fixtures::kitchen_sink();
    let mut body = save.snapshot.entities[0];
    body.id = sim::SimId(save.snapshot.entities.iter().map(|e| e.id.0).max().unwrap_or(0) + 1);
    body.kind = sim::Kind::Object(sim::module::ObjectKind::Creature, 9_999);
    body.sheet = None;
    let id = body.id;
    save.snapshot.entities.push(body);
    let npc = save.script.party.available.get_mut(&0).ok_or("no companion")?;
    npc.object = Some(id.0);
    save.script.party.members = vec![0];
    for level in [None, Some(&level)] {
        let unsheeted = crate::unsheeted(&save, level);
        if unsheeted != [0] {
            return Err(format!("unsheeted {unsheeted:?} with a level: {}", level.is_some()));
        }
        if crate::characters(&save, level).iter().any(|c| matches!(c.who, crate::Who::Companion { npc: 0, .. })) {
            return Err("the sheetless member is listed".into());
        }
    }
    Ok("member 0 unsheeted, with and without its level".into())
}

/// The installs, as kjk finds them from the environment.
fn discovery() -> vfs::Discovery {
    vfs::discover(|name| std::env::var(name).ok())
}

/// `game`'s data for edits: every item the install holds, a lightsaber a
/// weapon among them, and classes that use the Force.
fn data_of(game: vfs::Game) -> Result<String, String> {
    let discovery = discovery();
    if discovery.get(game).is_none() {
        return Ok(format!("skipped: no {} install", game.key()));
    }
    let data = crate::Data::load(&discovery, game)?;
    let saber = data
        .catalog
        .get("g_w_lghtsbr01")
        .ok_or("no g_w_lghtsbr01 among the items")?;
    if saber.weapon.is_none() {
        return Err("the lightsaber is no weapon".into());
    }
    let force = data.classes.classes.iter().filter(|class| class.force_die > 0).count();
    if data.catalog.len() < 500 || force == 0 {
        return Err(format!("{} items, {force} Force classes", data.catalog.len()));
    }
    Ok(format!(
        "{} items ({} unread), {} classes ({force} with the Force)",
        data.catalog.len(),
        data.unreadable.len(),
        data.classes.classes.len()
    ))
}

/// The fixtures' save with every kind of state in it, encoded.
fn sink_bytes() -> Vec<u8> {
    sim::fixtures::kitchen_sink().1.encode()
}

/// `bytes` with their format version set to `version` (after `KJKV`).
fn versioned(mut bytes: Vec<u8>, version: u32) -> Vec<u8> {
    bytes[4..8].copy_from_slice(&version.to_le_bytes());
    bytes
}

/// What `open_bytes` makes of each kind of save, named.
fn open_refusals() -> Result<String, String> {
    use crate::Refusal;
    let (now, oldest) = (sim::SAVE_FORMAT, sim::SAVE_OLDEST);
    let bytes = sink_bytes();
    let half = bytes[..bytes.len() / 2].to_vec();
    type Case = (&'static str, Vec<u8>, fn(&Refusal) -> bool);
    let cases: [Case; 3] = [
        ("newer", versioned(bytes.clone(), now + 3), |r| {
            matches!(r, Refusal::Newer { found, reads_up_to } if *found == sim::SAVE_FORMAT + 3 && *reads_up_to == sim::SAVE_FORMAT)
        }),
        ("older", versioned(bytes.clone(), oldest - 1), |r| {
            matches!(r, Refusal::Older { found, reads_from } if *found == sim::SAVE_OLDEST - 1 && *reads_from == sim::SAVE_OLDEST)
        }),
        ("cut", half, |r| matches!(r, Refusal::Damaged(_))),
    ];
    crate::open_bytes(&bytes).map_err(|why| format!("the fixture refused: {why}"))?;
    let mut told = Vec::new();
    for (name, bytes, fits) in cases {
        match crate::open_bytes(&bytes) {
            Ok(_) => return Err(format!("{name}: opened")),
            Err(refusal) if fits(&refusal) => told.push(format!("{name}: {refusal}")),
            Err(refusal) => return Err(format!("{name}: {refusal:?}")),
        }
    }
    let newer = crate::open_bytes(&versioned(bytes, now + 3)).err().map(|r| r.to_string());
    let both = newer.is_some_and(|text| text.contains(&(now + 3).to_string()) && text.contains(&now.to_string()));
    if !both {
        return Err(format!("the newer refusal does not name both versions: {told:?}"));
    }
    Ok(told.join("; "))
}

/// A save written to a folder opens from it; an empty folder is no save.
fn open_from_folder() -> Result<String, String> {
    let root = std::env::temp_dir().join(format!("kjk-edit-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let save = sim::fixtures::kitchen_sink().1;
    let id = saves::write(&root, None, &save, &saves::Meta::default())?;
    let opened = crate::open(&root, &id).map_err(|why| why.to_string())?;
    if opened.save != save || opened.id != id {
        return Err("opened another save".into());
    }
    std::fs::create_dir_all(root.join("0099")).map_err(|e| e.to_string())?;
    let empty = crate::open(&root, "0099");
    let _ = std::fs::remove_dir_all(&root);
    match empty {
        Err(crate::Refusal::NotASave(why)) => Ok(format!("{id} opened; 0099: {why}")),
        other => Err(format!("an empty folder: {:?}", other.map(|o| o.id))),
    }
}

/// An environment of `pairs` alone.
fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
    }
}

/// `want` as found, else what was.
fn expect<T: PartialEq + std::fmt::Debug>(found: T, want: T) -> Result<String, String> {
    if found == want {
        Ok(format!("{found:?}"))
    } else {
        Err(format!("{found:?}, not {want:?}"))
    }
}

const HOMES: [(&str, &str); 3] = [
    ("HOME", "/home/me"),
    ("APPDATA", "/appdata"),
    ("XDG_CONFIG_HOME", "/xdg"),
];

/// kjk's folder is the platform's own place for settings.
fn folder_platform() -> Result<String, String> {
    let base = if cfg!(target_os = "macos") {
        "/home/me/Library/Application Support"
    } else if cfg!(windows) {
        "/appdata"
    } else {
        "/xdg"
    };
    expect(store::kjk_folder(env(&HOMES)), Some(Path::new(base).join("kjk")))
}

/// Without a home there is no folder to tell.
fn folder_no_home() -> Result<String, String> {
    expect(store::kjk_folder(env(&[])), None)
}

/// `KJK_SAVES` wins, else `saves` beside the settings.
fn saves_folder() -> Result<String, String> {
    expect(
        saves::folder(env(&[("KJK_SAVES", "/mine")])),
        Some(PathBuf::from("/mine")),
    )?;
    expect(
        saves::folder(env(&HOMES)),
        store::kjk_folder(env(&HOMES)).map(|folder| folder.join("saves")),
    )
}
