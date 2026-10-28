//! `kjk materials <game>`: how every surface of every module's rooms, doors
//! and placeables would be drawn: surface kind, lighting path, env maps,
//! scrolling UVs, and what is not handled yet. Textures are read for their
//! format and TXI only, never decoded.

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use asset_odyssey::model::ModelIr;
use asset_odyssey::module::{Kind, ModuleIr};
use asset_odyssey::texture::TextureFacts;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

use crate::material::{self, Surface};
use crate::modules::{ResolverSource, module_names};
use crate::viewer;

/// What one texture name resolved to: facts, cube map, flip-book frames.
type Resolved = Option<(TextureFacts, bool, usize)>;

#[derive(Default)]
struct Tally {
    counts: BTreeMap<String, usize>,
    examples: BTreeMap<String, Vec<String>>,
}

impl Tally {
    fn add(&mut self, key: impl Into<String>, example: impl FnOnce() -> String) {
        let key = key.into();
        *self.counts.entry(key.clone()).or_insert(0) += 1;
        let examples = self.examples.entry(key).or_default();
        if examples.len() < 3 {
            let example = example();
            if !examples.contains(&example) {
                examples.push(example);
            }
        }
    }
}

pub fn run(discovery: &Discovery, game: &str) -> ExitCode {
    let Some(game) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("materials: use k1 or tsl");
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
    // The install's shared layers, opened once; each module stacks on them.
    let base = match Resolver::base(game, &index, TextureQuality::High) {
        Ok(base) => base,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut tally = Tally::default();
    let mut refused = Vec::new();
    let names = module_names(&index);
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
            Err(error) => {
                refused.push(format!("{name}: {error:?}"));
                continue;
            }
        };
        // Per module: an override or module archive can replace any of these.
        let mut models: BTreeMap<String, Option<ModelIr>> = BTreeMap::new();
        let mut textures: BTreeMap<String, Resolved> = BTreeMap::new();
        for area in &module.areas {
            let mut placed: Vec<(String, bool)> = area
                .rooms
                .iter()
                .map(|room| (room.model.clone(), true))
                .collect();
            placed.extend(area.instances.iter().filter_map(|instance| {
                matches!(instance.kind, Kind::Door | Kind::Placeable)
                    .then(|| instance.model.clone().map(|model| (model, false)))
                    .flatten()
            }));
            for (model_name, room) in placed {
                if !models.contains_key(&model_name) {
                    let loaded =
                        viewer::model(&resolver, &model_name).unwrap_or_else(|error| {
                            refused.push(format!("{name}: {error}"));
                            None
                        });
                    models.insert(model_name.clone(), loaded);
                }
                let Some(model) = &models[&model_name] else {
                    tally.add("model missing", || format!("{name}: {model_name}"));
                    continue;
                };
                for mesh in &model.meshes {
                    tally_mesh(
                        &mut tally,
                        &resolver,
                        &mut textures,
                        (name, &model_name, &model.nodes[mesh.node].name),
                        mesh,
                        room,
                    );
                }
            }
        }
    }
    outln!(
        "{}: {} modules, {} refused",
        game.key(),
        names.len(),
        refused.len()
    );
    for line in &refused {
        outln!("  refused {line}");
    }
    for (key, count) in &tally.counts {
        outln!("  {key}: {count} (e.g. {})", tally.examples[key].join("; "));
    }
    ExitCode::SUCCESS
}

fn tally_mesh(
    tally: &mut Tally,
    resolver: &Resolver,
    textures: &mut BTreeMap<String, Resolved>,
    (module, model, node): (&str, &str, &str),
    mesh: &asset_odyssey::model::MeshIr,
    room: bool,
) {
    let place = || format!("{module}: {model}/{node}");
    let mut resolve = |name: &str| -> Resolved {
        textures
            .entry(name.to_string())
            .or_insert_with(|| viewer::texture_facts(resolver, name))
            .clone()
    };
    let diffuse = mesh.texture.as_deref().map(|name| (name, resolve(name)));
    let lightmap = mesh.lightmap.as_deref().map(|name| (name, resolve(name)));
    for (role, texture) in [("diffuse", &diffuse), ("lightmap", &lightmap)] {
        if let Some((texture_name, None)) = texture {
            tally.add(format!("{role} texture missing"), || {
                format!("{module}: {texture_name}")
            });
        }
    }
    let facts: Vec<&TextureFacts> = [&diffuse, &lightmap]
        .into_iter()
        .flatten()
        .filter_map(|(_, resolved)| resolved.as_ref().map(|(facts, _, _)| facts))
        .collect();
    let surface = material::surface(mesh.transparency_hint, mesh.alpha, &facts);
    tally.add(format!("surface {}", surface.name()), place);
    if surface == Surface::Additive && mesh.transparency_hint == 0 {
        tally.add("additive without hint (transparent pass: deviation)", place);
    }
    let lit = lightmap
        .as_ref()
        .is_some_and(|(_, resolved)| resolved.is_some());
    let path = match (room, lit) {
        (true, true) => "lighting: room, diffuse x lightmap",
        (true, false) => "lighting: room, unlit",
        (false, true) => "lighting: object with lightmap",
        (false, false) => "lighting: object, ambient + lights",
    };
    tally.add(path, place);
    let mut odd: BTreeSet<String> = BTreeSet::new();
    for facts in &facts {
        if let Some(value) = &facts.odd_blending {
            odd.insert(value.clone());
        }
    }
    for value in odd {
        tally.add(
            format!("blending value xoreos would not match: {value:?}"),
            place,
        );
    }
    if let Some(envmap) = facts.iter().rev().find_map(|facts| facts.envmap.clone()) {
        let key = match resolve(&envmap) {
            Some((_, true, _)) => "env map: cube (drawn)",
            Some((_, false, _)) => "env map: 2D sphere map (not drawn)",
            None => "env map: missing",
        };
        tally.add(key, || format!("{module}: {envmap}"));
    }
    if let Some((texture_name, Some((_, _, frames)))) = &diffuse
        && *frames > 1
    {
        tally.add("diffuse flip-book (frame 0 only)", || {
            format!("{module}: {texture_name} ({frames} frames)")
        });
    }
    if mesh.uv_scroll.is_some() {
        tally.add("scrolling UVs", place);
    }
    if mesh.self_illum.iter().any(|&v| v > 0.0) {
        tally.add(
            if lit {
                "self-illumination on a lightmapped mesh"
            } else {
                "self-illumination"
            },
            place,
        );
    }
    if mesh.skinned {
        tally.add("skinned mesh (rest pose)", place);
    }
}
