//! The seat tokens a player's game keeps across its restarts, one for each
//! host it joined, in `seats.json` in the kjk folder.
//!
//! A host keeps the seat of a player whose connection timed out for that
//! seat's token (`net::session::RECONNECT_US`), and the world keeps the
//! companion that seat drove for it as long (`sim::world::drivers`). A game
//! that crashed and is started again had no token before, so it joined as
//! a new player. It took another seat, and its old seat and companion
//! stayed kept from it: it watched the party with no body, and after a
//! second crash the host refused it, saying every seat was taken (issue
//! 306). With the token kept here, the restarted game joins with it and the
//! host gives it back its own seat and companion.
//!
//! Only a player's game keeps and reads the file (`crash::PlayerRun`), so
//! scripted and headless runs join as they always did.

use std::path::{Path, PathBuf};

use bevy::prelude::warn;
use net::Peer;
use serde_json::{Map, Value};

/// The file's name in the kjk folder.
const FILE: &str = "seats.json";
/// How many hosts the file remembers: the ones joined last.
const HOSTS: usize = 16;

/// The file in the kjk folder, when there is one.
fn file() -> Option<PathBuf> {
    crate::settings::folder().map(|folder| folder.join(FILE))
}

/// The token this machine's game last held on `host`, if it kept one.
pub fn recall(host: &Peer) -> Option<u64> {
    recall_in(&file()?, host)
}

/// Keeps `token` as the one this game holds on `host`.
pub fn keep(host: &Peer, token: u64) {
    if let Some(path) = file()
        && let Err(error) = keep_in(&path, host, token, now_secs())
    {
        warn!(target: "net", %error, "the seat's token was not kept");
    }
}

/// The seconds since the Unix epoch, which order the hosts by when each
/// was joined.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The file's entries, or none when it is missing or does not read.
fn entries(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_else(Map::new)
}

/// The token the file at `path` keeps for `host`.
fn recall_in(path: &Path, host: &Peer) -> Option<u64> {
    entries(path)
        .get(&host.to_string())?
        .get("token")?
        .as_u64()
        .filter(|token| *token != 0)
}

/// Writes `token` for `host` into the file at `path`, joined at `at`. The
/// file keeps the `HOSTS` hosts joined last.
fn keep_in(path: &Path, host: &Peer, token: u64, at: u64) -> std::io::Result<()> {
    let mut entries = entries(path);
    entries.insert(
        host.to_string(),
        serde_json::json!({ "token": token, "at": at }),
    );
    let mut hosts: Vec<(String, u64)> = entries
        .iter()
        .map(|(host, entry)| {
            (
                host.clone(),
                entry.get("at").and_then(Value::as_u64).unwrap_or(0),
            )
        })
        .collect();
    hosts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (old, _) in hosts.into_iter().skip(HOSTS) {
        entries.remove(&old);
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(entries))
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    store::write_atomically(path, text.as_bytes())
}

/// The probes of the file (`kjk sim probes`, group "seat tokens").
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    let folder = std::env::temp_dir().join(format!("kjk-seat-tokens-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let path = folder.join(FILE);
    let host = |n: u16| Peer::Addr(std::net::SocketAddr::from(([10, 0, 0, 1], n)));
    let missing = recall_in(&path, &host(1));
    let kept = keep_in(&path, &host(1), 41, 100).map(|()| recall_in(&path, &host(1)));
    let replaced = keep_in(&path, &host(1), 42, 101).map(|()| recall_in(&path, &host(1)));
    let pruned = (2..=u16::try_from(HOSTS).unwrap_or(16) + 1)
        .try_for_each(|n| keep_in(&path, &host(n), u64::from(n), 101 + u64::from(n)))
        .map(|()| {
            (
                recall_in(&path, &host(1)),
                recall_in(&path, &host(2)),
                entries(&path).len(),
            )
        });
    let _ = std::fs::write(&path, b"{\"10.0.0.1:1\": {\"tok");
    let unreadable = recall_in(&path, &host(1));
    let _ = std::fs::remove_dir_all(&folder);
    vec![
        (
            "none before a join",
            check(missing.is_none(), format!("{missing:?}")),
        ),
        (
            "kept and read back",
            check(matches!(kept, Ok(Some(41))), format!("{kept:?}")),
        ),
        (
            "a new token replaces the old",
            check(matches!(replaced, Ok(Some(42))), format!("{replaced:?}")),
        ),
        (
            "only the hosts joined last",
            check(
                matches!(pruned, Ok((None, Some(2), count)) if count == HOSTS),
                format!("{pruned:?}"),
            ),
        ),
        (
            "a file cut short reads as none",
            check(unreadable.is_none(), format!("{unreadable:?}")),
        ),
    ]
}
