//! `kjk formats <game>`: parses every copy of every resource the install
//! ships (BIFs, every archive, loose files), and for formats with a writer
//! checks that writing a parsed file back gives the same bytes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;
use fmt_odyssey::lip::{self, Lip};
use fmt_odyssey::mdl::{self, Model};
use fmt_odyssey::ssf::Ssf;
use fmt_odyssey::texture::Texture;
use fmt_odyssey::tlk::{self, Tlk};
use fmt_odyssey::twoda::TwoDa;
use vfs::archive::Archive;
use vfs::chitin::Chitin;
use vfs::resolver::Location;
use vfs::{DirIndex, Discovery, Game};

/// Extensions that name a GFF. A copy with one of these whose bytes are not
/// a GFF is counted as a failure rather than skipped.
const GFF_TYPES: &[&str] = &[
    "are", "bic", "btc", "btd", "bte", "bti", "btm", "btp", "bts", "btt", "dlg", "fac", "gff",
    "gic", "git", "gui", "ifo", "itp", "jrl", "pth", "utc", "utd", "ute", "uti", "utm", "utp",
    "uts", "utt", "utw",
];

/// Saved games are the player's data and are skipped, as in `check`.
const PLAYER_FOLDERS: &[&str] = &["saves/", "cloudsaves/"];
const ARCHIVE_EXTENSIONS: &[&str] = &["erf", "mod", "rim", "sav", "hak"];

struct Copy {
    /// Where it came from, e.g. `chitin.key` or `modules/end_m01aa.rim`.
    source: String,
    name: String,
    restype: ResType,
    location: Location,
}

#[derive(Default)]
struct Tally {
    copies: usize,
    parsed: usize,
    same: usize,
    failures: Vec<String>,
    /// Written back in another order than the file's own, with the same tree:
    /// the file was laid out by a different writer.
    differs: Vec<String>,
    /// Written back, and reading that back gives a different tree: data lost.
    lossy: Vec<String>,
    /// Written back differently, and the file's tables hold something a tree
    /// cannot represent (see `gff_audit`).
    irregular: Vec<String>,
    /// Structs where a repeated label holds different values.
    ambiguous_labels: usize,
    /// Facts about formats that are read but not written (TLK).
    notes: Vec<String>,
    /// Counts by variant (texture pixel formats, cube maps, …), with the
    /// first few examples of each.
    kinds: BTreeMap<String, (usize, Vec<String>)>,
}

/// Every copy of every resource in the install, in no particular order.
fn inventory(index: &DirIndex) -> Result<Vec<Copy>, String> {
    let mut copies = Vec::new();
    let chitin = Chitin::load(index).map_err(|error| error.to_string())?;
    for entry in &chitin.entries {
        let path = chitin.bifs[entry.bif]
            .path
            .clone()
            .expect("resolved entries have a BIF");
        copies.push(Copy {
            source: "chitin.key".into(),
            name: entry.resref.clone(),
            restype: entry.restype,
            location: Location::on_disk(path, u64::from(entry.offset), u64::from(entry.size)),
        });
    }
    for (key, path) in index.iter() {
        if PLAYER_FOLDERS.iter().any(|dir| key.starts_with(dir)) {
            continue;
        }
        let Some((stem, extension)) = key
            .rsplit('/')
            .next()
            .and_then(|name| name.rsplit_once('.'))
        else {
            continue;
        };
        if ARCHIVE_EXTENSIONS.contains(&extension) {
            let archive = Archive::open(path).map_err(|error| format!("{key}: {error}"))?;
            for entry in archive.entries {
                copies.push(Copy {
                    source: key.to_string(),
                    name: entry.resref,
                    restype: entry.restype,
                    location: Location::on_disk(
                        path.to_path_buf(),
                        u64::from(entry.offset),
                        u64::from(entry.size),
                    ),
                });
            }
        } else if (key.starts_with("override/") || !key.contains('/'))
            && let Some(restype) = ResType::from_extension(extension)
        {
            copies.push(loose(key, stem, restype, path));
        }
    }
    Ok(copies)
}

fn loose(key: &str, stem: &str, restype: ResType, path: &Path) -> Copy {
    Copy {
        source: key
            .rsplit_once('/')
            .map_or("(root)", |(dir, _)| dir)
            .to_string(),
        name: stem.to_string(),
        restype,
        location: Location::on_disk(
            PathBuf::from(path),
            0,
            std::fs::metadata(path).map_or(0, |meta| meta.len()),
        ),
    }
}

/// The first byte offset where two byte strings differ.
fn first_difference(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}

pub fn run(discovery: &Discovery, game: &str) -> ExitCode {
    if Game::from_key(game) == Some(Game::JediAcademy) {
        return crate::formats_ja::run(discovery);
    }
    let Some(game) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("formats: use k1, tsl or ja");
        return ExitCode::from(2);
    };
    let Some(install) = discovery.get(game) else {
        eprintln!("{}: not found (set {})", game.key(), game.env_var());
        return ExitCode::FAILURE;
    };
    let copies = match DirIndex::build(&install.data_root)
        .map_err(|error| error.to_string())
        .and_then(|index| inventory(&index))
    {
        Ok(copies) => copies,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mdx_type = ResType::MDX;
    let mdx_of: BTreeMap<(&str, &str), &Copy> = copies
        .iter()
        .filter(|copy| copy.restype == mdx_type)
        .map(|copy| ((copy.source.as_str(), copy.name.as_str()), copy))
        .collect();
    // The games look an MDX up by name, so an MDL's MDX can live in another
    // archive (K1 keeps `rims/chargen.rim`'s in `rims/chargendx.rim`).
    let mdx_by_name: BTreeMap<&str, &Copy> = copies
        .iter()
        .filter(|copy| copy.restype == mdx_type)
        .map(|copy| (copy.name.as_str(), copy))
        .collect();
    let texture_names: std::collections::BTreeSet<&str> = copies
        .iter()
        .filter(|copy| matches!(copy.restype.extension(), Some("tpc" | "tga")))
        .map(|copy| copy.name.as_str())
        .collect();
    for copy in &copies {
        let bytes = match copy.location.read() {
            Ok(bytes) => bytes,
            Err(error) => {
                let tally = tallies.entry(copy.restype.to_string()).or_default();
                tally.copies += 1;
                tally
                    .failures
                    .push(format!("{} {}: {error}", copy.source, copy.name));
                continue;
            }
        };
        let extension = copy.restype.to_string();
        let looks_2da = bytes.starts_with(b"2DA ");
        let looks_gff = bytes.len() >= 8 && &bytes[4..8] == b"V3.2";
        if (extension == "2da" && !looks_2da)
            || (GFF_TYPES.contains(&extension.as_str()) && !looks_gff)
        {
            let tally = tallies.entry(format!("{extension} (content)")).or_default();
            tally.copies += 1;
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(8)]).into_owned();
            tally.failures.push(format!(
                "{} {}.{extension}: content starts {head:?}",
                copy.source, copy.name
            ));
            continue;
        }
        if extension == "lyt" || extension == "vis" {
            let tally = tallies
                .entry(format!("{extension} (text, read only)"))
                .or_default();
            tally.copies += 1;
            tally.parsed += 1;
            let mut note = |key: String| {
                let slot = tally.kinds.entry(key).or_insert((0, Vec::new()));
                slot.0 += 1;
                if slot.1.len() < 2 {
                    slot.1.push(copy.name.clone());
                }
            };
            if extension == "lyt" {
                match fmt_odyssey::lyt::Layout::parse(&bytes) {
                    Ok(layout) => {
                        note(format!(
                            "rooms {}",
                            if layout.rooms.is_empty() {
                                "none"
                            } else {
                                "some"
                            }
                        ));
                        if !layout.door_hooks.is_empty() {
                            note("has door hooks".into());
                        }
                        if !layout.tracks.is_empty() || !layout.obstacles.is_empty() {
                            note("has tracks or obstacles".into());
                        }
                        for line in &layout.unknown {
                            note(format!("line outside known sections: {line}"));
                        }
                    }
                    Err(error) => {
                        tally.parsed -= 1;
                        tally
                            .failures
                            .push(format!("{} {}: {error}", copy.source, copy.name));
                    }
                }
            } else {
                let vis = fmt_odyssey::vis::Visibility::parse(&bytes);
                if vis.rooms.is_empty() {
                    note("no rooms".into());
                }
                if !vis.count_mismatches.is_empty() {
                    note("room count differs from its list".into());
                }
            }
            continue;
        }
        if matches!(extension.as_str(), "wok" | "pwk" | "dwk") {
            let tally = tallies
                .entry(format!("{extension} (walkmesh, read only)"))
                .or_default();
            tally.copies += 1;
            match fmt_odyssey::bwm::Walkmesh::parse(&bytes) {
                Ok(walkmesh) => {
                    tally.parsed += 1;
                    let mut note = |key: String| {
                        let slot = tally.kinds.entry(key).or_insert((0, Vec::new()));
                        slot.0 += 1;
                        if slot.1.len() < 2 {
                            slot.1.push(copy.name.clone());
                        }
                    };
                    note(format!("area walkmesh: {}", walkmesh.area));
                    if walkmesh.faces.is_empty() {
                        note("no faces".into());
                    }
                    let nodes = walkmesh.aabb.len() as u32;
                    let bad_child = walkmesh.aabb.iter().any(|entry| {
                        entry.face == -1 && entry.children.iter().any(|&c| c >= nodes)
                    });
                    if bad_child {
                        note("AABB child index outside the tree".into());
                    }
                    let edges = walkmesh.faces.len() as i32 * 3;
                    if walkmesh
                        .adjacency
                        .iter()
                        .any(|row| row.iter().any(|&e| e < -1 || e >= edges))
                    {
                        note("adjacency outside the faces".into());
                    }
                }
                Err(error) => tally.failures.push(format!(
                    "{} {}.{extension}: {error}",
                    copy.source, copy.name
                )),
            }
            continue;
        }
        if extension == "mdl" {
            let tally = tallies
                .entry("mdl (model, read only)".to_string())
                .or_default();
            tally.copies += 1;
            let mdx = match mdx_of
                .get(&(copy.source.as_str(), copy.name.as_str()))
                .or_else(|| mdx_by_name.get(copy.name.as_str()))
            {
                Some(mdx) => match mdx.location.read() {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        tally
                            .failures
                            .push(format!("{} {}.mdx: {error}", copy.source, copy.name));
                        continue;
                    }
                },
                None => Vec::new(),
            };
            match Model::parse(&bytes, &mdx) {
                Ok(model) => {
                    tally.parsed += 1;
                    let mut note = |key: String| {
                        let slot = tally.kinds.entry(key).or_insert((0, Vec::new()));
                        slot.0 += 1;
                        if slot.1.len() < 2 {
                            slot.1.push(copy.name.clone());
                        }
                    };
                    note(format!("classification {:#04x}", model.classification));
                    for node in model.nodes() {
                        let headers = [
                            mdl::FLAG_LIGHT,
                            mdl::FLAG_EMITTER,
                            mdl::FLAG_REFERENCE,
                            mdl::FLAG_MESH,
                        ]
                        .iter()
                        .filter(|&&flag| node.flags & flag != 0)
                        .count();
                        if headers > 1 {
                            note(format!(
                                "node with {headers} of light/emitter/reference/mesh"
                            ));
                        }
                        if node.flags & mdl::FLAG_CAMERA != 0 {
                            note("node with camera flag".into());
                        }
                        let q = node.orientation;
                        let norm = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
                        if (norm - 1.0).abs() > 0.01 {
                            note(format!("rest orientation not unit ({norm:.3})"));
                        }
                        for controller in &node.controllers {
                            if controller.times.windows(2).any(|t| t[1] < t[0]) {
                                note(format!("controller {} times go backwards", controller.kind));
                            }
                        }
                        if let Some(mesh) = &node.mesh {
                            let b = mesh.bounds;
                            let slack = 0.01
                                + 0.001
                                    * (b[3] - b[0])
                                        .abs()
                                        .max((b[4] - b[1]).abs())
                                        .max((b[5] - b[2]).abs());
                            let outside = mesh.positions.iter().any(|p| {
                                !p.iter().all(|v| v.is_finite())
                                    || p[0] < b[0] - slack
                                    || p[1] < b[1] - slack
                                    || p[2] < b[2] - slack
                                    || p[0] > b[3] + slack
                                    || p[1] > b[4] + slack
                                    || p[2] > b[5] + slack
                            });
                            if outside {
                                note("vertex outside its mesh bounds".into());
                            }
                            let length =
                                |n: &[f32; 3]| (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                            if mesh.normals.iter().any(|n| length(n) < 0.01) {
                                note("mesh with zero normals".into());
                            }
                            if mesh
                                .normals
                                .iter()
                                .any(|n| length(n) >= 0.01 && (length(n) - 1.0).abs() > 0.05)
                            {
                                note("mesh with other non-unit normals".into());
                            }
                            let from_faces: Vec<u16> =
                                mesh.faces.iter().flat_map(|f| f.vertices).collect();
                            if !mesh.indices.is_empty() && mesh.indices != from_faces {
                                note("index array differs from face vertices".into());
                            }
                            let limit = mesh.positions.len() as u16;
                            if !mesh.saber
                                && mesh
                                    .faces
                                    .iter()
                                    .any(|f| f.vertices.iter().any(|&v| v >= limit))
                            {
                                note("face index beyond vertex count".into());
                            }
                            if mesh.skin.is_some() {
                                note("skin mesh node".into());
                            }
                            if mesh
                                .animate_uv
                                .is_some_and(|uv| uv[0] != 0.0 || uv[1] != 0.0)
                            {
                                note("mesh with animateuv".into());
                            }
                        }
                    }
                    // Skinned meshes: the file's inverse binds (qbones/tbones)
                    // against the ones the rest pose gives.
                    let ir = asset_odyssey::model::ModelIr::from_model(&model);
                    for skin in ir.meshes.iter().filter_map(|mesh| mesh.skin.as_ref()) {
                        let worst = skin
                            .inverse_binds
                            .iter()
                            .zip(&skin.rest_inverse_binds)
                            .flat_map(|(a, b)| {
                                a.iter()
                                    .flatten()
                                    .zip(b.iter().flatten())
                                    .map(|(x, y)| (x - y).abs())
                            })
                            .fold(0.0f32, f32::max);
                        note(if worst <= 0.01 {
                            "skin: file inverse binds = rest pose (within 0.01)".into()
                        } else if worst <= 0.1 {
                            "skin: file inverse binds within 0.1 of rest pose".into()
                        } else {
                            "skin: file inverse binds differ from rest pose".into()
                        });
                    }
                    if mdx.is_empty() && model.nodes().iter().any(|n| n.mesh.is_some()) {
                        note("meshes without an mdx".into());
                    }
                    let ir = asset_odyssey::model::ModelIr::from_model(&model);
                    note(format!(
                        "converted: {}",
                        if ir.meshes.is_empty() {
                            "no drawable mesh"
                        } else {
                            "has drawable meshes"
                        }
                    ));
                    for (_, refusal) in &ir.refusals {
                        note(format!("refused mesh: {refusal:?}"));
                    }
                    for mesh in &ir.meshes {
                        if let Some(texture) = &mesh.texture
                            && !texture_names.contains(texture.as_str())
                        {
                            note("drawable mesh names a texture the install lacks".into());
                        }
                    }
                }
                Err(error) => tally
                    .failures
                    .push(format!("{} {}.mdl: {error}", copy.source, copy.name)),
            }
            continue;
        }
        if extension == "tpc" || extension == "tga" {
            let tally = tallies
                .entry(format!("{extension} (texture, read only)"))
                .or_default();
            tally.copies += 1;
            let parsed = if extension == "tpc" {
                Texture::parse_tpc(&bytes)
            } else {
                Texture::parse_tga(&bytes)
            };
            match parsed {
                Ok(texture) => {
                    tally.parsed += 1;
                    let key = format!(
                        "{:?}{}{}{}{}",
                        texture.format,
                        if texture.cube { " cube" } else { "" },
                        if texture.layers.len() > 1 && !texture.cube {
                            " frames"
                        } else {
                            ""
                        },
                        if texture.missing_mips > 0 {
                            " missing-mips"
                        } else {
                            ""
                        },
                        if texture.declared_size.is_some() {
                            " size-mismatch"
                        } else {
                            ""
                        }
                    );
                    let slot = tally.kinds.entry(key).or_insert((0, Vec::new()));
                    slot.0 += 1;
                    if slot.1.len() < 2 {
                        slot.1.push(match texture.declared_size {
                            Some((declared, computed)) => format!(
                                "{} {}x{} x{} declared {declared} computed {computed}",
                                copy.name,
                                texture.width(),
                                texture.height(),
                                texture.layers.len()
                            ),
                            None => copy.name.clone(),
                        });
                    }
                    if texture.rgba(0, 0).is_none() {
                        tally.failures.push(format!(
                            "{} {}: first mip does not decode",
                            copy.source, copy.name
                        ));
                    }
                }
                Err(error) => tally.failures.push(format!(
                    "{} {}.{extension}: {error}",
                    copy.source, copy.name
                )),
            }
            continue;
        }
        if bytes.starts_with(b"TLK ") {
            let tally = tallies
                .entry(format!("{} (tlk, read only)", copy.restype))
                .or_default();
            tally.copies += 1;
            match Tlk::parse(&bytes) {
                Ok(table) => {
                    tally.parsed += 1;
                    let with_text = table
                        .entries
                        .iter()
                        .filter(|entry| entry.flags & tlk::TEXT_PRESENT != 0)
                        .count();
                    tally.notes.push(format!(
                        "{} {}: language {}, {} entries, {with_text} with text",
                        copy.source,
                        copy.name,
                        table.language,
                        table.entries.len()
                    ));
                }
                Err(error) => tally
                    .failures
                    .push(format!("{} {}: {error}", copy.source, copy.name)),
            }
            continue;
        }
        if bytes.starts_with(b"SSF ") {
            let tally = tallies
                .entry(format!("{} (ssf)", copy.restype))
                .or_default();
            tally.copies += 1;
            let label = format!("{} {}.{}", copy.source, copy.name, copy.restype);
            match Ssf::parse(&bytes) {
                Ok(set) => {
                    tally.parsed += 1;
                    if set.write() == bytes {
                        tally.same += 1;
                    } else {
                        tally.lossy.push(label);
                    }
                }
                Err(error) => tally.failures.push(format!("{label}: {error}")),
            }
            continue;
        }
        if bytes.starts_with(b"LIP ") {
            let tally = tallies
                .entry(format!("{} (lip, read only)", copy.restype))
                .or_default();
            tally.copies += 1;
            match Lip::parse(&bytes) {
                Ok(lip) if lip.keys.iter().all(|&(_, shape)| shape < lip::SHAPES) => {
                    tally.parsed += 1;
                }
                Ok(_) => tally.failures.push(format!(
                    "{} {}: a shape past {}",
                    copy.source,
                    copy.name,
                    lip::SHAPES - 1
                )),
                Err(error) => tally
                    .failures
                    .push(format!("{} {}: {error}", copy.source, copy.name)),
            }
            continue;
        }
        if looks_2da {
            let tally = tallies
                .entry(format!("{} (2da)", copy.restype))
                .or_default();
            tally.copies += 1;
            let label = format!("{} {}.{}", copy.source, copy.name, copy.restype);
            match TwoDa::parse(&bytes) {
                Ok(table) => {
                    tally.parsed += 1;
                    let written = table.write();
                    if written == bytes {
                        tally.same += 1;
                    } else if TwoDa::parse(&written).as_ref() != Ok(&table) {
                        tally.lossy.push(label);
                    } else {
                        tally.differs.push(format!(
                            "{label}: {} vs {} bytes, first difference at {}",
                            written.len(),
                            bytes.len(),
                            first_difference(&written, &bytes)
                        ));
                    }
                }
                Err(error) => tally.failures.push(format!("{label}: {error}")),
            }
            continue;
        }
        if !looks_gff {
            continue;
        }
        let tally = tallies
            .entry(format!("{} (gff)", copy.restype))
            .or_default();
        tally.copies += 1;
        let label = format!("{} {}.{}", copy.source, copy.name, copy.restype);
        match Gff::parse(&bytes) {
            Ok(gff) => {
                tally.parsed += 1;
                let written = gff.write();
                if written == bytes {
                    tally.same += 1;
                } else {
                    let line = format!(
                        "{label}: {} vs {} bytes, first difference at {}",
                        written.len(),
                        bytes.len(),
                        first_difference(&written, &bytes)
                    );
                    if Gff::parse(&written).as_ref() != Ok(&gff) {
                        tally.lossy.push(line);
                    } else {
                        match fmt_odyssey::gff_audit::audit(&bytes) {
                            Ok(audit) if !audit.is_regular() => {
                                tally.ambiguous_labels += audit.repeated_with_different_values;
                                tally.irregular.push(format!("{line}; {audit:?}"));
                            }
                            _ => tally.differs.push(line),
                        }
                    }
                }
            }
            Err(error) => tally.failures.push(format!("{label}: {error}")),
        }
    }

    outln!("{}: {} resource copies", game.key(), copies.len());
    let mut failed = false;
    for (kind, tally) in &tallies {
        outln!(
            "  {kind}: {} copies, {} parsed, {} failed | written back: {} identical, {} other layout, {} irregular, {} lossy",
            tally.copies,
            tally.parsed,
            tally.failures.len(),
            tally.same,
            tally.differs.len(),
            tally.irregular.len(),
            tally.lossy.len()
        );
        if tally.ambiguous_labels > 0 {
            outln!(
                "    {} structs repeat a label with different values",
                tally.ambiguous_labels
            );
        }
        for line in &tally.notes {
            outln!("    {line}");
        }
        for (kind, (count, examples)) in &tally.kinds {
            outln!("    {kind}: {count} (e.g. {})", examples.join("; "));
        }
        for line in tally.failures.iter().take(3) {
            outln!("    failed {line}");
        }
        for line in tally.irregular.iter().take(2) {
            outln!("    irregular {line}");
        }
        for line in tally.differs.iter().take(2) {
            outln!("    other layout {line}");
        }
        for line in tally.lossy.iter().take(5) {
            outln!("    LOSSY {line}");
        }
        failed |= !tally.failures.is_empty() || !tally.lossy.is_empty();
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
