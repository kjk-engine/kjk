//! The probes of `copies` (`kjk sim probes`).

use super::*;
use crate::{Meta, RECEIVED, list, read};
use sim::probe::{Checks, check};
use std::path::Path;

const BUILD: &str = "probe-build";

/// The round trip, in a folder of its own under the system's temporary
/// one, for `kjk sim probes`:
/// - a save the host wrote is packed, kept by a player byte for byte as
///   `r…`, marked and loadable;
/// - another build's copy, an unreadable world and a stranger's bytes are
///   refused with nothing written;
/// - a later copy of the same save replaces the earlier;
/// - the player's own saves, quick included, are untouched;
/// - two hosts of one name keep a copy each of the same slot;
/// - a host's copies past `KEPT` go, the oldest first, another's stay;
/// - with no number left after the highest, a save and a copy are refused;
/// - ids stay inside the folder.
pub fn probes() -> Checks {
    let root = std::env::temp_dir().join(format!("kjk-save-copies-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut checks = ids();
    checks.extend(run(&root).unwrap_or_else(|why| vec![("copies_scenario", Err(why))]));
    let _ = std::fs::remove_dir_all(&root);
    checks
}

/// Which ids name a save: a copy's `r` and digits, a playtest checkpoint's
/// game and digits, nothing that leaves the folder, nor a save being
/// written or set aside.
fn ids() -> Checks {
    let cases = [
        ("quick", true),
        ("0001", true),
        ("r0001", true),
        ("r", false),
        ("rx1", false),
        ("r../0001", false),
        ("../0001", false),
        ("", false),
        (".0001.new", false),
        (".r0001.old", false),
        ("k1-03", true),
        ("tsl-12", true),
        ("k1-", false),
        ("-01", false),
        ("K1-01", false),
        ("k1-01-02", false),
        ("k1-../01", false),
        ("../k1-01", false),
        (".k1-01.new", false),
    ];
    let wrong: Vec<&str> = cases
        .iter()
        .filter(|(id, valid)| crate::valid_id(id) != *valid)
        .map(|(id, _)| *id)
        .collect();
    vec![(
        "received_ids",
        check(
            wrong.is_empty(),
            format!("{} ids, misjudged {wrong:?}", cases.len()),
        ),
    )]
}

/// A copy's origin: the host's install, its name, the save's id there.
fn by(install: &str, from: &str, slot: &str) -> Received {
    Received {
        host: install.into(),
        from: from.into(),
        slot: slot.into(),
        build: BUILD.into(),
    }
}

fn run(root: &Path) -> Result<Checks, String> {
    let level = sim::determinism::proving_ground();
    let mut world = sim::SimWorld::new(sim::determinism::Setup::pinned().world, level.clone())
        .map_err(|error| error.to_string())?;
    let snapshot = world.snapshot();
    let meta = Meta {
        name: "bridge".into(),
        module: world.level().id().name,
        area: "here".into(),
        saved: 1,
        seats: snapshot.seats(),
        ..Meta::default()
    };
    let save = sim::Save {
        hz: sim::TICK_HZ,
        level: world.level().id(),
        script: world.script_state().clone(),
        snapshot: snapshot.clone(),
    };
    let world_of = |root: &Path, id: &str| {
        std::fs::read(root.join(id).join(crate::WORLD)).map_err(|error| error.to_string())
    };
    let (host, player) = (&root.join("host"), &root.join("player"));
    let from = |slot: &str| by("ana", "Ana", slot);
    // The player's own saves first.
    let own = [
        crate::write(player, Some(crate::QUICK), &save, &meta)?,
        crate::write(player, None, &save, &meta)?,
    ];
    let own_world = |root: &Path| -> Result<Vec<Vec<u8>>, String> {
        own.iter().map(|id| world_of(root, id)).collect()
    };
    let own_before = own_world(player)?;
    // The host's quick save and a numbered one, each sent as a copy.
    let mut sent = Vec::new();
    for slot in [crate::QUICK, "0001"] {
        let id = crate::write(host, Some(slot), &save, &meta)?;
        let bytes = world_of(host, &id)?;
        sent.push((bytes.clone(), pack(&meta, &bytes, &from(&id))));
    }
    let kept: Vec<String> = sent
        .iter()
        .map(|(_, copy)| receive(player, copy, BUILD))
        .collect::<Result<_, _>>()?;
    let identical = sent
        .iter()
        .zip(&kept)
        .all(|((bytes, _), id)| world_of(player, id).is_ok_and(|kept| kept == *bytes));
    let listing = list(player);
    let marked = kept
        .iter()
        .all(|id| listing.received.get(id).is_some_and(|r| r.from == "Ana"));
    let (again, _) = read(player, &kept[0])?;
    let loaded = sim::SimWorld::from_snapshot(&again.snapshot, level)
        .map(|mut world| world.snapshot().hash() == snapshot.hash());
    let count = || list(player).saves.len();
    let before = count();
    let renamed = Meta {
        name: "bridge again".into(),
        ..meta.clone()
    };
    let replaced = receive(
        player,
        &pack(&renamed, &sent[0].0, &from(crate::QUICK)),
        BUILD,
    );
    let replaced_ok = replaced.as_deref() == Ok(kept[0].as_str())
        && count() == before
        && read(player, &kept[0]).is_ok_and(|(_, meta)| meta.name == "bridge again");
    let other_build = receive(player, &sent[1].1, "another-build");
    let mut unreadable_world = sent[1].0.clone();
    unreadable_world[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    let unreadable = receive(
        player,
        &pack(&meta, &unreadable_world, &from("0009")),
        BUILD,
    );
    let stranger = receive(player, b"not a copy at all", BUILD);
    let own_after = own_world(player)?;
    let refused = |result: &Result<String, String>| result.is_err() && count() == before;
    let mut checks = vec![
        (
            "byte_identical",
            check(identical, format!("kept as {kept:?}, byte for byte")),
        ),
        (
            "marked_received",
            check(
                marked && kept.iter().all(|id| id.starts_with(RECEIVED)),
                format!("{:?}", listing.received),
            ),
        ),
        (
            "loads_again",
            check(
                matches!(loaded, Ok(true)),
                format!("made again from the copy: {loaded:?}"),
            ),
        ),
        (
            "same_slot_replaced",
            check(replaced_ok, format!("{replaced:?}, {} saves", count())),
        ),
        (
            "own_saves_untouched",
            check(
                own_after == own_before && kept.iter().all(|id| !own.contains(id)),
                format!("own {own:?}, copies {kept:?}"),
            ),
        ),
        (
            "other_build_refused",
            check(
                refused(&other_build)
                    && other_build
                        .as_ref()
                        .is_err_and(|why| why.contains("another-build") && why.contains(BUILD)),
                format!("{other_build:?}"),
            ),
        ),
        (
            "unreadable_refused",
            check(refused(&unreadable), format!("{unreadable:?}")),
        ),
        (
            "stranger_refused",
            check(refused(&stranger), format!("{stranger:?}")),
        ),
    ];
    // Another install's host, called Ana too, saving in the same slot.
    let bob = receive(
        player,
        &pack(&meta, &sent[0].0, &by("bob", "Ana", crate::QUICK)),
        BUILD,
    );
    let two_hosts = bob.as_ref().is_ok_and(|id| *id != kept[0])
        && count() == before + 1
        && origin(player, &kept[0]).is_some_and(|copy| copy.host == "ana")
        && read(player, &kept[0]).is_ok_and(|(_, meta)| meta.name == "bridge again");
    checks.push((
        "two_hosts_same_slot",
        check(two_hosts, format!("Bob's {bob:?} beside Ana's {}", kept[0])),
    ));
    checks.extend(capped(&root.join("capped"), &sent[0].0, &meta)?);
    checks.extend(exhausted(&root.join("full"), &save, &meta, &sent[0].1)?);
    Ok(checks)
}

/// `KEPT` + 2 copies from Cid into `root`, after one of Ana's older than
/// all: Cid's two oldest go, Ana's stays.
fn capped(root: &Path, world: &[u8], meta: &Meta) -> Result<Checks, String> {
    let at = |saved: u64| Meta {
        saved,
        ..meta.clone()
    };
    let ana = receive(root, &pack(&at(0), world, &by("ana", "Ana", "0001")), BUILD)?;
    let sent = KEPT as u64 + 2;
    for n in 1..=sent {
        let slot = format!("{n:04}");
        receive(root, &pack(&at(n), world, &by("cid", "Cid", &slot)), BUILD)?;
    }
    let left = list(root).received;
    let cids: Vec<&str> = left
        .values()
        .filter(|copy| copy.host == "cid")
        .map(|copy| copy.slot.as_str())
        .collect();
    let latest: Vec<String> = (3..=sent).map(|n| format!("{n:04}")).collect();
    Ok(vec![(
        "copies_capped",
        check(
            cids == latest && left.contains_key(&ana),
            format!("{sent} of Cid's sent, kept {cids:?}; Ana's {ana} kept"),
        ),
    )])
}

/// A save and a copy into `root` whose highest numbers are the last a
/// `u32` holds: refused, with nothing written.
fn exhausted(root: &Path, save: &sim::Save, meta: &Meta, copy: &[u8]) -> Result<Checks, String> {
    let last = u32::MAX.to_string();
    for id in [last.clone(), format!("{RECEIVED}{last}")] {
        std::fs::create_dir_all(root.join(id)).map_err(|error| error.to_string())?;
    }
    let own = crate::write(root, None, save, meta);
    let received = receive(root, copy, BUILD);
    let entries = std::fs::read_dir(root).map_or(0, |entries| entries.count());
    Ok(vec![(
        "numbers_exhausted",
        check(
            own.is_err() && received.is_err() && entries == 2,
            format!("{own:?}, {received:?}, {entries} folders"),
        ),
    )])
}
