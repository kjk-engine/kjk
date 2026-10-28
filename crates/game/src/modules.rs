//! `kjk modules <game> [module]`: every module (or one) turned into a
//! `ModuleIr`; coverage for all, a full dump for one.

use std::collections::BTreeMap;
use std::process::ExitCode;

use asset_odyssey::creature;
use asset_odyssey::module::{Kind, Missing, ModuleIr, Source, Title};
use fmt_odyssey::ResType;
use fmt_odyssey::tlk::Tlk;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

pub struct ResolverSource<'a>(pub &'a Resolver);

impl Source for ResolverSource<'_> {
    fn read(&self, resref: &str, restype: ResType) -> Option<Vec<u8>> {
        self.0.find(resref, restype)?.location.read().ok()
    }

    fn exists(&self, resref: &str, restype: ResType) -> bool {
        self.0.find(resref, restype).is_some()
    }
    fn names(&self, restype: ResType) -> Vec<String> {
        self.0.names(restype)
    }

    fn own_names(&self, restype: ResType) -> Vec<String> {
        self.0.module_names(restype)
    }

    fn voice(&self, resref: &str) -> Option<Vec<u8>> {
        self.0.voice(resref)?.read().ok()
    }

    fn talk(&self) -> Option<std::sync::Arc<Tlk>> {
        self.0.talk.get()
    }
}

/// Module names: each `modules/*.rim` that is not `_s.rim`, and each
/// `modules/*.mod`.
pub fn module_names(index: &DirIndex) -> Vec<String> {
    let mut names: Vec<String> = index
        .files_in("modules")
        .into_iter()
        .filter_map(|(name, _)| {
            let stem = name
                .strip_suffix(".rim")
                .or_else(|| name.strip_suffix(".mod"))?;
            (!stem.ends_with("_s")).then(|| stem.to_string())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

fn mdl_type() -> ResType {
    ResType::MDL
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn run(discovery: &Discovery, game: &str, only: Option<&str>) -> ExitCode {
    let Some(game) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("modules: use k1 or tsl");
        return ExitCode::from(2);
    };
    let Some(install) = discovery.get(game) else {
        eprintln!("{}: not found (set {})", game.key(), game.env_var());
        return ExitCode::FAILURE;
    };
    let index = match DirIndex::build(&install.data_root) {
        Ok(index) => index,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let names = match only {
        Some(name) => vec![name.to_ascii_lowercase()],
        None => module_names(&index),
    };
    let tlk = index
        .get("dialog.tlk")
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| Tlk::parse(&bytes).ok());

    // The install's shared layers, opened once; each module stacks on them.
    let base = match Resolver::base(game, &index, TextureQuality::High) {
        Ok(base) => base,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut refused = Vec::new();
    let mut converted = 0;
    let mut totals: BTreeMap<String, usize> = BTreeMap::new();
    let mut missing_kinds: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    for name in &names {
        let resolver = match base.with_module(&index, name) {
            Ok(resolver) => resolver,
            Err(error) => {
                refused.push(format!("{name}: {error}"));
                continue;
            }
        };
        let module = match ModuleIr::build(name, &ResolverSource(&resolver)) {
            Ok(module) => module,
            Err(refusal) => {
                refused.push(format!("{name}: {refusal:?}"));
                continue;
            }
        };
        converted += 1;
        if only.is_some() {
            dump(&module, tlk.as_ref());
        }
        let mut tables = asset_odyssey::tables::Tables::default();
        for area in &module.areas {
            *totals.entry("areas".into()).or_insert(0) += 1;
            *totals.entry("rooms".into()).or_insert(0) += area.rooms.len();
            *totals.entry("cameras".into()).or_insert(0) += area.cameras.len();
            *totals.entry("rooms only in ARE".into()).or_insert(0) += area.rooms_only_in_are.len();
            *totals.entry("rooms only in LYT".into()).or_insert(0) += area.rooms_only_in_lyt.len();
            for instance in area.instances.iter().filter(|i| i.kind == Kind::Creature) {
                let source = ResolverSource(&resolver);
                let key = match creature::resolve_with(&source, &instance.template, &mut tables) {
                    Ok(look) => {
                        let exists = |name: &str| source.exists(name, mdl_type());
                        if let Some((from_uti, from_base_item)) = &look.bodyvar_disagrees {
                            let slot = missing_kinds
                                .entry("creature bodyvar UTI/base item differ".into())
                                .or_insert((0, Vec::new()));
                            slot.0 += 1;
                            if slot.1.len() < 3 {
                                slot.1.push(format!(
                                    "{}/{}: {from_uti} vs {from_base_item} ({:?})",
                                    module.name, instance.template, look.armour
                                ));
                            }
                        }
                        if look.backup_head {
                            *totals
                                .entry("creature head from backuphead".into())
                                .or_insert(0) += 1;
                        }
                        if !exists(&look.body) {
                            format!("creature body model missing ({})", look.model_type)
                        } else if look.head.as_deref().is_some_and(|head| !exists(head)) {
                            "creature head model missing".to_string()
                        } else {
                            format!("creature model type {}", look.model_type)
                        }
                    }
                    Err(refusal) => format!("creature refused: {refusal:?}")
                        .split('(')
                        .next()
                        .unwrap_or("")
                        .to_string(),
                };
                *totals.entry(key).or_insert(0) += 1;
            }
            for instance in &area.instances {
                *totals.entry(format!("{:?}", instance.kind)).or_insert(0) += 1;
                if instance.model.is_some() {
                    *totals
                        .entry(format!("{:?} with model", instance.kind))
                        .or_insert(0) += 1;
                }
                if instance.kind == Kind::Door && instance.appearance.is_some_and(|a| a != 0) {
                    *totals.entry("Door appearance not 0".into()).or_insert(0) += 1;
                }
            }
            // VIS links listed one way only: xoreos uses the lists as
            // written, reone adds the reverse of every link.
            for room in &area.rooms {
                for other in &room.visible {
                    *totals.entry("VIS links".into()).or_insert(0) += 1;
                    let back = area.rooms.iter().any(|candidate| {
                        candidate.model.eq_ignore_ascii_case(other)
                            && candidate
                                .visible
                                .iter()
                                .any(|name| name.eq_ignore_ascii_case(&room.model))
                    });
                    if !back {
                        *totals.entry("VIS links one way".into()).or_insert(0) += 1;
                        let slot = missing_kinds
                            .entry("VIS reverse link".into())
                            .or_insert((0, Vec::new()));
                        slot.0 += 1;
                        if slot.1.len() < 3 {
                            slot.1
                                .push(format!("{}: {} -> {other}", module.name, room.model));
                        }
                    }
                }
            }
            for missing in &area.missing {
                let key = match missing {
                    Missing::RoomModel(_) => "room model".to_string(),
                    Missing::RoomWalkmesh(_) => "room walkmesh".to_string(),
                    Missing::Template(kind, _) => format!("{kind:?} template"),
                    Missing::Layout => "layout".to_string(),
                    Missing::Visibility => "visibility".to_string(),
                    Missing::InstanceModel(kind, _) => format!("{kind:?} model"),
                };
                let slot = missing_kinds.entry(key).or_insert((0, Vec::new()));
                slot.0 += 1;
                if slot.1.len() < 3 {
                    slot.1
                        .push(format!("{}/{}: {missing:?}", module.name, area.resref));
                }
            }
        }
    }
    outln!(
        "{}: {} modules, {converted} converted, {} refused",
        game.key(),
        names.len(),
        refused.len()
    );
    for line in &refused {
        outln!("  refused {line}");
    }
    for (key, count) in &totals {
        outln!("  {key}: {count}");
    }
    for (key, (count, examples)) in &missing_kinds {
        outln!("  missing {key}: {count} (e.g. {})", examples.join("; "));
    }
    if refused.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn dump(module: &ModuleIr, tlk: Option<&Tlk>) {
    let title = match &module.title {
        Title::StrRef(strref) => tlk
            .and_then(|table| table.text(*strref))
            .map_or(format!("strref {strref}"), |bytes| {
                format!("{:?}", text(bytes))
            }),
        Title::Text(bytes) => format!("{:?}", text(bytes)),
        Title::None => "(none)".to_string(),
    };
    outln!("module {} {title}", module.name);
    outln!(
        "  entry: area {} at {:?} facing {:?}",
        module.entry_area,
        module.entry_position,
        module.entry_direction
    );
    for area in &module.areas {
        let name = area
            .name_strref
            .and_then(|strref| tlk.and_then(|table| table.text(strref)))
            .map_or(String::new(), |bytes| format!(" {:?}", text(bytes)));
        outln!("  area {}{name} tag {:?}", area.resref, text(&area.tag));
        outln!("    rooms: {}", area.rooms.len());
        for room in &area.rooms {
            outln!(
                "      {} at {:?}, sees {} rooms, walkmesh {}",
                room.model,
                room.position,
                room.visible.len(),
                room.has_walkmesh
            );
        }
        for kind in Kind::ALL {
            let of_kind: Vec<_> = area.instances.iter().filter(|i| i.kind == kind).collect();
            if of_kind.is_empty() {
                continue;
            }
            outln!("    {kind:?}: {}", of_kind.len());
            for instance in of_kind {
                let mut line = format!(
                    "      {} tag {:?} at [{:.2}, {:.2}, {:.2}] facing {:.2}",
                    instance.template,
                    text(&instance.tag),
                    instance.position[0],
                    instance.position[1],
                    instance.position[2],
                    instance.facing
                );
                if let Some((module, tag)) = &instance.linked_to
                    && !(module.is_empty() && tag.is_empty())
                {
                    line += &format!(" -> {module}:{}", text(tag));
                }
                if !instance.geometry.is_empty() {
                    line += &format!(", {} outline points", instance.geometry.len());
                }
                if let Some(trap) = &instance.trap {
                    line += &format!(
                        ", trap row {} (its own DCs: detect {}, disarm {}; one-shot {}), faction {:?}",
                        trap.trap_type,
                        trap.detect_dc,
                        trap.disarm_dc,
                        trap.one_shot,
                        instance.faction
                    );
                }
                if !instance.template_found {
                    line += " (template missing)";
                }
                outln!("{line}");
            }
        }
        outln!("    cameras: {}", area.cameras.len());
        outln!("    missing: {:?}", area.missing);
        if !area.rooms_only_in_are.is_empty() || !area.rooms_only_in_lyt.is_empty() {
            outln!(
                "    rooms only in ARE {:?}, only in LYT {:?}",
                area.rooms_only_in_are,
                area.rooms_only_in_lyt
            );
        }
    }
}
