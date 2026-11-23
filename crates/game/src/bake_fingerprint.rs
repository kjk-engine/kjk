//! The fingerprint of a bake: which game files a set of baked lightmaps was
//! made from, so that the game shows a bake only over those files.
//!
//! Players run the games unpatched, KOTOR 2 with TSLRCM, KOTOR 1 with the K1
//! Community Patch, or with other mods. TSLRCM changes the placeables of
//! about 11 areas and the rooms of 2. The K1 Community Patch changes the
//! rooms' geometry or the layout of their lightmaps in 18 areas, and the
//! placeables of about 37 modules. A bake made over one set of files would
//! be drawn wrongly over another.
//!
//! The fingerprint has three parts.
//!
//! - `rooms` is a digest of what the lightmaps depend on. That is the
//!   area's layout (each room's model and its place), each room model's
//!   MDL and MDX, and each stored lightmap that the rooms name, with its
//!   TXI. The game reads the stored lightmaps for the fill and for the
//!   levels of the fitted lights, so a changed stored lightmap changes the
//!   bake too.
//! - `placeables` is a digest of the placeables the bake lights at their
//!   vertices or lets block its rays. These are the placeables with a mesh
//!   that has no skin and does not dangle (`placeable_light::lit_meshes`),
//!   in the area's order. Each one counts with its number among the area's
//!   instances, its model's name, its place and its bearing as stored, and
//!   its model's MDL and MDX.
//! - `format` is `FORMAT`, a number that is raised by hand when the baker's
//!   files change their meaning.
//!
//! Every file is the one the games would read, after the game folder,
//! `override/` and the Steam Workshop items have been ranked. Scripts,
//! dialogue, sounds and the diffuse textures are left out, because the bake
//! does not depend on them, or because a texture mod would then turn every
//! bake away. Nothing depends on where the files lie, on their times, or on
//! the machine.
//!
//! The baker writes the fingerprint into `FILE` in each set it writes, and
//! `kjk detail-bake <module> --stamp <folder>` writes it into a set baked
//! earlier. The game computes the fingerprint when an area loads under the
//! bake and compares the two (`verdict`).

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use asset_odyssey::model::ModelIr;
use asset_odyssey::module::{AreaIr, Direction, Kind};
use fmt_odyssey::ResType;
use vfs::resolver::Resolver;

/// The file in a bake's set that holds its fingerprint.
pub const FILE: &str = "fingerprint.txt";

/// The meaning of the bake's files. Raise it by hand when the baker writes
/// something the game must read differently, so that every older bake is
/// turned away until it is baked again.
///
/// Format 2 (23 November 2026): `placeables.txt`, `probes.txt` and
/// `probes-sun.txt` hold their light without the fill from the stored
/// lightmaps, and name the stored texel the game reads that fill from
/// (`stored_fill`). They carry this number in a `format` line too, so that
/// a set without `fingerprint.txt` is checked as well.
pub const FORMAT: u32 = 2;

/// The fingerprint of one area's files, or of the files a bake was made
/// from. The two digests are hexadecimal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// The area's name (its ARE), in lower case.
    pub area: String,
    pub format: u32,
    pub rooms: String,
    pub placeables: String,
}

/// What the game may take from a bake whose fingerprint was compared with
/// the player's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Everything matches. The game takes the whole bake.
    Whole,
    /// The rooms match, but the placeables do not. The game takes the
    /// lightmaps, and the placeables keep the light probes.
    LightmapsOnly,
    /// The rooms or the format differ. The game takes nothing from the
    /// bake. The text says why.
    Refused(String),
}

/// A digest fed with fields that each say how long they are, so that two
/// different lists of fields never give the same stream of bytes.
struct Digest(blake3::Hasher);

impl Digest {
    fn new(kind: &str) -> Digest {
        let mut digest = Digest(blake3::Hasher::new());
        digest.text(kind);
        digest
    }

    fn text(&mut self, text: &str) {
        self.0.update(&(text.len() as u64).to_le_bytes());
        self.0.update(text.as_bytes());
    }

    /// A file's bytes, or a mark that the file is missing.
    fn file(&mut self, bytes: Option<&[u8]>) {
        match bytes {
            Some(bytes) => {
                self.0.update(&[1]);
                self.0.update(&(bytes.len() as u64).to_le_bytes());
                self.0.update(bytes);
            }
            None => {
                self.0.update(&[0]);
            }
        }
    }

    fn numbers(&mut self, numbers: &[f32]) {
        for number in numbers {
            self.0.update(&number.to_bits().to_le_bytes());
        }
    }

    fn count(&mut self, count: u64) {
        self.0.update(&count.to_le_bytes());
    }

    /// The first 128 bits, in hexadecimal.
    fn finish(self) -> String {
        self.0.finalize().to_hex()[..32].to_string()
    }
}

/// A model's MDL and MDX as the games would read them.
fn model_files(digest: &mut Digest, resolver: &Resolver, name: &str) {
    digest.text(name);
    digest.file(resolver.read(name, ResType::MDL).as_deref());
    digest.file(resolver.read(name, ResType::MDX).as_deref());
}

/// A model as the game reads it (`viewer::parsed`), or `None` when it is
/// missing or does not read.
fn model_ir(resolver: &Resolver, name: &str) -> Option<ModelIr> {
    crate::viewer::parsed(resolver, name)
        .ok()
        .flatten()
        .map(|parsed| ModelIr::from_model(&parsed))
}

/// The fingerprint of area `area` in the files that `resolver` reads.
pub fn of_area(resolver: &Resolver, area: &AreaIr) -> Fingerprint {
    Fingerprint {
        area: area.resref.to_ascii_lowercase(),
        format: FORMAT,
        rooms: rooms(resolver, area),
        placeables: placeables(resolver, area),
    }
}

/// The digest of the layout, the room models and their stored lightmaps.
fn rooms(resolver: &Resolver, area: &AreaIr) -> String {
    let mut digest = Digest::new("rooms");
    digest.count(area.rooms.len() as u64);
    let mut models = BTreeSet::new();
    for room in &area.rooms {
        let name = room.model.to_ascii_lowercase();
        digest.text(&name);
        digest.numbers(&room.position);
        models.insert(name);
    }
    let mut lightmaps = BTreeSet::new();
    for name in &models {
        model_files(&mut digest, resolver, name);
        if let Some(ir) = model_ir(resolver, name) {
            lightmaps.extend(
                (ir.meshes.iter())
                    .filter_map(|mesh| mesh.lightmap.as_deref())
                    .map(str::to_ascii_lowercase),
            );
        }
    }
    // Each stored lightmap as the game picks it: a TPC or a TGA, whichever
    // the highest layer holds, with its TXI.
    for name in &lightmaps {
        digest.text(name);
        match resolver.find_any(name, &[ResType::TPC, ResType::TGA]) {
            Some((restype, found)) => {
                digest.text(if restype == ResType::TPC {
                    "tpc"
                } else {
                    "tga"
                });
                digest.file(found.location.read().ok().as_deref());
            }
            None => digest.text("missing"),
        }
        digest.file(resolver.read(name, ResType::TXI).as_deref());
    }
    digest.finish()
}

/// The digest of the placeables the bake lights or lets block its rays, in
/// the order of the area's instances, as `still_placeables` of the baker and
/// `module_scene` of the game find them.
fn placeables(resolver: &Resolver, area: &AreaIr) -> String {
    let mut digest = Digest::new("placeables");
    let mut lit_of: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
    for (object, instance) in area.instances.iter().enumerate() {
        if instance.kind != Kind::Placeable {
            continue;
        }
        let Some(name) = instance.model.as_deref().map(str::to_ascii_lowercase) else {
            continue;
        };
        let lit = *lit_of.entry(name.clone()).or_insert_with(|| {
            model_ir(resolver, &name).is_some_and(|ir| {
                crate::placeable_light::lit_meshes(&ir)
                    .iter()
                    .any(|lit| *lit)
            })
        });
        if !lit {
            continue;
        }
        digest.count(object as u64);
        digest.text(&name);
        digest.numbers(&instance.position);
        match instance.direction {
            Direction::Bearing(bearing) => {
                digest.text("bearing");
                digest.numbers(&[bearing]);
            }
            Direction::Vector(vector) => {
                digest.text("vector");
                digest.numbers(&vector);
            }
        }
    }
    for (name, _) in lit_of.iter().filter(|(_, lit)| **lit) {
        model_files(&mut digest, resolver, name);
    }
    digest.finish()
}

/// What the game may take from a bake stamped `stamp`, over files whose
/// fingerprint is `current`.
pub fn verdict(stamp: &Fingerprint, current: &Fingerprint) -> Verdict {
    if stamp.format != current.format {
        Verdict::Refused(format!(
            "the bake's format is {}, and this game reads format {}",
            stamp.format, current.format
        ))
    } else if stamp.rooms != current.rooms {
        Verdict::Refused(
            "the area's rooms or stored lightmaps in the game's files are not the ones the bake was made from"
                .to_string(),
        )
    } else if stamp.placeables != current.placeables {
        Verdict::LightmapsOnly
    } else {
        Verdict::Whole
    }
}

/// The text of `FILE` for `print`.
pub fn text(print: &Fingerprint) -> String {
    let mut text = String::from(
        "# The game files this set of lightmaps was baked from (crates/game/src/bake_fingerprint.rs).\n\
         # format is the meaning of the bake's files. rooms is a digest of the area's layout, its room\n\
         # models and their stored lightmaps. placeables is a digest of the placeables the bake lights\n\
         # or lets block its rays. The game shows the lightmaps only when format and rooms match the\n\
         # player's files, and the placeables' light only when placeables matches too.\n",
    );
    let _ = writeln!(text, "area {}", print.area);
    let _ = writeln!(text, "format {}", print.format);
    let _ = writeln!(text, "rooms {}", print.rooms);
    let _ = writeln!(text, "placeables {}", print.placeables);
    text
}

/// Writes `print` into `FILE` of the set `folder`.
pub fn write(folder: &Path, print: &Fingerprint) -> Result<(), String> {
    let path = folder.join(FILE);
    std::fs::write(&path, text(print)).map_err(|error| format!("{}: {error}", path.display()))
}

/// The fingerprint in `FILE` of the set `folder`. `None` when the set has
/// no such file, and the reason when the file does not read.
pub fn read(folder: &Path) -> Option<Result<Fingerprint, String>> {
    let path = folder.join(FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    Some(parse(&text).ok_or_else(|| format!("{} does not read", path.display())))
}

/// The fingerprint in the text of a `FILE`.
fn parse(text: &str) -> Option<Fingerprint> {
    let (mut area, mut format, mut rooms, mut placeables) = (None, None, None, None);
    for line in text.lines().map(str::trim) {
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let value = value.trim().to_string();
        match key {
            "area" => area = Some(value.to_ascii_lowercase()),
            "format" => format = value.parse().ok(),
            "rooms" => rooms = Some(value),
            "placeables" => placeables = Some(value),
            _ => {}
        }
    }
    Some(Fingerprint {
        area: area?,
        format: format?,
        rooms: rooms?,
        placeables: placeables?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_reads_back() {
        let print = Fingerprint {
            area: "101per".into(),
            format: FORMAT,
            rooms: "0123456789abcdef0123456789abcdef".into(),
            placeables: "fedcba9876543210fedcba9876543210".into(),
        };
        assert_eq!(parse(&text(&print)), Some(print));
    }

    #[test]
    fn a_verdict_follows_the_parts() {
        let stamp = Fingerprint {
            area: "a".into(),
            format: FORMAT,
            rooms: "r".into(),
            placeables: "p".into(),
        };
        assert_eq!(verdict(&stamp, &stamp), Verdict::Whole);
        let moved = Fingerprint {
            placeables: "q".into(),
            ..stamp.clone()
        };
        assert_eq!(verdict(&stamp, &moved), Verdict::LightmapsOnly);
        let rebuilt = Fingerprint {
            rooms: "s".into(),
            ..stamp.clone()
        };
        assert!(matches!(verdict(&stamp, &rebuilt), Verdict::Refused(_)));
        let older = Fingerprint {
            format: FORMAT + 1,
            ..stamp.clone()
        };
        assert!(matches!(verdict(&stamp, &older), Verdict::Refused(_)));
    }
}
