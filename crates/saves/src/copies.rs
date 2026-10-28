//! Copies of a host's saves on its players' machines (I30 P17). At every
//! save the host makes, each player gets it over the network (net's
//! `SaveCopy`) and keeps it here among their own saves, marked with where it
//! came from, as `r0001`, `r0002`, … (never `quick`, the player's own). Any
//! of them can host the campaign from it later with the normal Load. A
//! host is known by its install's id (`Received::host`), never by its name,
//! which every host shares: a later copy of the same save from the same
//! host replaces the earlier, and only the `KEPT` latest copies of each
//! host stay. A copy is the save's meta and its world's bytes as the host
//! wrote them (`save.kjkv`, kept byte for byte):
//!
//! ```text
//! copy = "KJKC", version u32, meta length u32, meta.json (with "received"),
//!        the world in LZ4, its size first
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Meta, RECEIVED, list, next_id, parse_meta, write_files};

const MAGIC: &[u8; 4] = b"KJKC";
const VERSION: u32 = 1;
/// The largest world a copy unpacks to; a copy saying more is refused
/// before anything is allocated.
const MAX_WORLD: usize = 64 * 1024 * 1024;
/// The most copies kept from one host; the oldest past it are dropped.
pub const KEPT: usize = 10;

/// Where a save on this machine came from, when another's game made it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Received {
    /// The host's install (made once at random, kept in its settings):
    /// whose copies are whose.
    pub host: String,
    /// Whose game it is, as the list shows it: the host's character's
    /// name, else the name the host plays under.
    pub from: String,
    /// The save's id on the host (`quick`, `0003`): a later copy of the
    /// same one from the same host replaces this.
    pub slot: String,
    /// The build that made it (`KJK_GIT_SHA`).
    pub build: String,
}

/// A save written here (`meta` and its world's bytes) as a copy for the
/// players, saying where it came from.
pub fn pack(meta: &Meta, world: &[u8], received: &Received) -> Vec<u8> {
    let text =
        serde_json::to_vec(&crate::meta_file(meta, Some(received))).expect("a meta serializes");
    let mut out = Vec::with_capacity(12 + text.len() + world.len() / 2);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(&text);
    out.extend_from_slice(&lz4_flex::compress_prepend_size(world));
    out
}

/// What a copy holds: its meta, where it came from, its world's bytes.
pub fn unpack(copy: &[u8]) -> Result<(Meta, Received, Vec<u8>), String> {
    let refused = || "not a save copy".to_string();
    let rest = copy.strip_prefix(MAGIC.as_slice()).ok_or_else(refused)?;
    let (version, rest) = rest.split_first_chunk::<4>().ok_or_else(refused)?;
    let version = u32::from_le_bytes(*version);
    if version != VERSION {
        return Err(format!(
            "a save copy of version {version}; this build reads {VERSION}"
        ));
    }
    let (len, rest) = rest.split_first_chunk::<4>().ok_or_else(refused)?;
    let (text, world) = rest
        .split_at_checked(u32::from_le_bytes(*len) as usize)
        .ok_or_else(refused)?;
    let text = std::str::from_utf8(text).map_err(|_| refused())?;
    let (meta, received) = parse_meta(text)?;
    let received = received.ok_or("a save copy that does not say where it came from")?;
    let (size, _) = world.split_first_chunk::<4>().ok_or_else(refused)?;
    if u32::from_le_bytes(*size) as usize > MAX_WORLD {
        return Err(format!("a save copy of more than {} MiB", MAX_WORLD >> 20));
    }
    let world = lz4_flex::decompress_size_prepended(world).map_err(|_| refused())?;
    Ok((meta, received, world))
}

/// Keeps `copy` in `root` among this machine's saves, if this build
/// (`build`) made it and reads its world. It replaces an earlier copy of
/// the same save from the same host, else takes the next `r` number, and
/// the host's oldest copies past `KEPT` are dropped. Its id, or why not,
/// with nothing written (or why an older copy could not be dropped).
pub fn receive(root: &Path, copy: &[u8], build: &str) -> Result<String, String> {
    let (meta, received, world) = unpack(copy)?;
    let (from, name) = (&received.from, &meta.name);
    if received.build != build {
        return Err(format!(
            "{from}'s save \"{name}\" was made by build {}, this is build {build}: not kept",
            received.build
        ));
    }
    sim::Save::decode(&world)
        .map_err(|error| format!("{from}'s save \"{name}\" does not read here: {error}"))?;
    let earlier = list(root)
        .received
        .into_iter()
        .find(|(_, earlier)| earlier.host == received.host && earlier.slot == received.slot);
    let id = match earlier {
        Some((id, _)) => id,
        None => next_id(root, RECEIVED)?,
    };
    write_files(root, &id, &world, &meta, Some(&received))?;
    drop_oldest(root, &received.host, &id).map_err(|why| {
        format!("{from}'s save \"{name}\" was kept as {id}, but an older copy stays: {why}")
    })?;
    Ok(id)
}

/// The copies from `host` past the `KEPT` latest removed, never `kept`
/// (the one just written).
fn drop_oldest(root: &Path, host: &str, kept: &str) -> Result<(), String> {
    let listing = list(root);
    let theirs = listing.saves.iter().map(|(id, _)| id).filter(|id| {
        *id != kept
            && listing
                .received
                .get(*id)
                .is_some_and(|copy| copy.host == host)
    });
    for id in theirs.skip(KEPT - 1) {
        crate::delete(root, id)?;
    }
    Ok(())
}

/// Where the save `id` came from, if it is a copy.
pub fn origin(root: &Path, id: &str) -> Option<Received> {
    list(root).received.remove(id)
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
