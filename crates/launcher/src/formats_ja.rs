//! `kjk formats ja`: every Ghoul2 model and skeleton, animation table, saber,
//! shader and effect file in Jedi Academy's PK3s, parsed and cross-checked.

use std::collections::BTreeMap;
use std::process::ExitCode;

use fmt_ja::animation_cfg::AnimationConfig;
use fmt_ja::ghoul2::{Gla, Glm, SURFACE_BOLT};
use fmt_ja::{script, shader};
use vfs::ja::JaStack;
use vfs::{DirIndex, Discovery, Game};

#[derive(Default)]
struct Tally {
    copies: usize,
    parsed: usize,
    failures: Vec<String>,
    notes: BTreeMap<String, (usize, Vec<String>)>,
}

impl Tally {
    fn note(&mut self, key: impl Into<String>, example: &str) {
        let slot = self.notes.entry(key.into()).or_insert((0, Vec::new()));
        slot.0 += 1;
        if slot.1.len() < 2 {
            slot.1.push(example.to_string());
        }
    }
}

pub fn run(discovery: &Discovery) -> ExitCode {
    let Some(install) = discovery.get(Game::JediAcademy) else {
        eprintln!("ja: not found (set KJK_JA)");
        return ExitCode::FAILURE;
    };
    let stack = match DirIndex::build(&install.data_root) {
        Ok(index) => JaStack::open(&index),
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let all = stack.all();
    let read = |name: &str| -> Option<Vec<u8>> {
        let resolved = all.get(name)?;
        stack.read(name, resolved.source).ok()
    };

    let mut tallies: BTreeMap<&str, Tally> = BTreeMap::new();
    let mut gla_frames: BTreeMap<String, usize> = BTreeMap::new();
    let names: Vec<&str> = all.keys().copied().collect();
    for name in &names {
        let extension = name.rsplit_once('.').map_or("", |(_, e)| e);
        let kind = match extension {
            "glm" | "gla" | "sab" | "shader" | "efx" => extension,
            "cfg" if name.ends_with("animation.cfg") => "animation.cfg",
            _ => continue,
        };
        let tally = tallies.entry(kind).or_default();
        tally.copies += 1;
        let Some(bytes) = read(name) else {
            tally.failures.push(format!("{name}: unreadable"));
            continue;
        };
        match kind {
            "glm" => match Glm::parse(&bytes) {
                Ok(glm) => {
                    tally.parsed += 1;
                    tally.note(format!("LODs {}", glm.lods.len()), name);
                    if glm.surfaces.iter().any(|s| s.flags & SURFACE_BOLT != 0) {
                        tally.note("has bolt surfaces (tags)", name);
                    }
                }
                Err(error) => tally.failures.push(format!("{name}: {error}")),
            },
            "gla" => match Gla::parse(&bytes) {
                Ok(gla) => {
                    tally.parsed += 1;
                    let stem = name.trim_end_matches(".gla").to_string();
                    gla_frames.insert(stem, gla.frame_count);
                    if gla.bones.is_empty() {
                        tally.note("no bones", name);
                    }
                    let off_unit = gla
                        .pool
                        .iter()
                        .filter(|entry| {
                            let (q, _) = fmt_ja::ghoul2::unpack_bone(entry);
                            let norm =
                                (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
                            (norm - 1.0).abs() > 0.01
                        })
                        .count();
                    tally.note(
                        if off_unit == 0 {
                            "every pool rotation is a unit quaternion"
                        } else {
                            "pool rotations off unit length"
                        },
                        &format!(
                            "{name}: {} bones, {} frames, {} pool entries, {off_unit} off",
                            gla.bones.len(),
                            gla.frame_count,
                            gla.pool.len()
                        ),
                    );
                }
                Err(error) => tally.failures.push(format!("{name}: {error}")),
            },
            "animation.cfg" => {
                tally.parsed += 1;
                let config = AnimationConfig::parse(&bytes);
                if !config.bad_lines.is_empty() {
                    tally.note("lines that are not a sequence", name);
                }
            }
            "sab" | "efx" => match script::parse(&bytes) {
                Ok(root) => {
                    tally.parsed += 1;
                    if !root.lines.is_empty() {
                        let what = if kind == "efx" {
                            "top-level settings (e.g. repeatDelay)"
                        } else {
                            "words outside any block"
                        };
                        tally.note(what, name);
                    }
                }
                Err(error) => tally.failures.push(format!("{name}: {error}")),
            },
            "shader" => match shader::parse(&bytes) {
                Ok(file) => {
                    tally.parsed += 1;
                    for _ in &file.shaders {
                        tally.note("shader blocks", name);
                    }
                    for refusal in &file.refused {
                        tally.note(format!("refused block: {}", refusal.reason), &refusal.name);
                    }
                    if !file.stray.is_empty() {
                        tally.note("words outside any shader", name);
                    }
                }
                Err(error) => tally.failures.push(format!("{name}: {error}")),
            },
            _ => {}
        }
    }

    // Cross-checks: each animation table fits its skeleton; each saber's
    // hilt model loads and carries blade tags.
    let mut checks = Tally::default();
    for name in names.iter().filter(|n| n.ends_with("/animation.cfg")) {
        let folder = name.trim_end_matches("animation.cfg");
        let Some(frames) = gla_frames
            .iter()
            .find(|(stem, _)| stem.starts_with(folder) && !stem[folder.len()..].contains('/'))
            .map(|(_, frames)| *frames)
        else {
            checks.note("animation.cfg with no GLA beside it", name);
            continue;
        };
        let config = AnimationConfig::parse(&read(name).unwrap_or_else(Vec::new));
        let outside = config
            .sequences
            .iter()
            .filter(|s| {
                s.first_frame < 0 || (s.first_frame + s.frame_count.max(0)) as usize > frames
            })
            .count();
        checks.note(
            if outside == 0 {
                "animation.cfg within its GLA"
            } else {
                "animation.cfg past its GLA"
            },
            &format!(
                "{name}: {} sequences, {frames} frames, {outside} outside",
                config.sequences.len()
            ),
        );
    }
    for name in names.iter().filter(|n| n.ends_with(".sab")) {
        let Ok(root) = script::parse(&read(name).unwrap_or_else(Vec::new)) else {
            continue;
        };
        for saber in &root.children {
            let label = saber.name.join(" ");
            let model = saber
                .lines
                .iter()
                .find(|line| {
                    line.first()
                        .is_some_and(|k| k.eq_ignore_ascii_case("saberModel"))
                })
                .and_then(|line| line.get(1))
                .map(|model| fmt_ja::game_path(model));
            let Some(model) = model else {
                checks.note("saber without saberModel", &label);
                continue;
            };
            match read(&model).map(|bytes| Glm::parse(&bytes)) {
                None => checks.note("saber model not found", &format!("{label} → {model}")),
                Some(Err(error)) => checks.note(
                    "saber model does not parse",
                    &format!("{label} → {model}: {error}"),
                ),
                Some(Ok(glm)) => {
                    let blades = glm
                        .surfaces
                        .iter()
                        .filter(|s| s.name.to_ascii_lowercase().starts_with("*blade"))
                        .count();
                    checks.note(
                        if blades > 0 {
                            "saber hilt with blade tags"
                        } else {
                            "saber hilt without blade tags"
                        },
                        &format!("{label} → {model} ({blades} blade tags)"),
                    );
                }
            }
        }
    }

    let mut failed = false;
    for (kind, tally) in tallies
        .iter()
        .chain(std::iter::once((&"cross-checks", &checks)))
    {
        outln!(
            "  {kind}: {} files, {} parsed, {} failed",
            tally.copies,
            tally.parsed,
            tally.failures.len()
        );
        for (key, (count, examples)) in &tally.notes {
            outln!("    {key}: {count} (e.g. {})", examples.join("; "));
        }
        for failure in tally.failures.iter().take(5) {
            outln!("    failed {failure}");
        }
        failed |= !tally.failures.is_empty();
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
