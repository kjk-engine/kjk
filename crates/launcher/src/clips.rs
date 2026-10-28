//! `kjk clips <game>`: for every creature in every module, which of the
//! basic clips its body finds through its supermodel chain (reone's names:
//! `cpause1`/`cwalk`/`crun` for creature types S and L, `pause1`/`walk`/
//! `run`/`tlknorm` otherwise; a creature type lacking a `c…` clip falls
//! back to the shared name, `viewer::creature_clip`), how many of each
//! clip's tracks bind to an animated node of the body, and which bodies
//! scale animation positions by something other than 1 (where xoreos and
//! reone would differ); the animations its own scripts play (literal
//! `ANIMATION_…` constants) and whether its body has each one's clip; its
//! body's emitters; and how each lies once dead (`death_census`). For every
//! placeable, where its model finds the clip its look loops (`own_fx`): its
//! own, through its supermodel chain, or nowhere.

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use asset_odyssey::creature;
use asset_odyssey::model::ModelIr;
use asset_odyssey::module::{Kind, ModuleIr};
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

use crate::modules::{ResolverSource, module_names};
use crate::viewer;

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

/// How a body is drawn (`viewer`'s test): retargeted as JA's humanoid, or
/// on its own clips; and the census's label for it.
fn drawn(body: &ModelIr) -> (bool, &'static str) {
    match asset_ja::rig::is_humanoid(body) {
        true => (true, "humanoid"),
        false => (false, "own clips"),
    }
}

/// How creature `template` of module `module` (its `look` and `body`) lies
/// once dead, as the views draw it (`cast`): a ragdoll if its body fits one,
/// its dying clip where it dies in place (`sim::combat::death::in_place`),
/// and for a body on its own clips the dying and dead clips (`found`, by
/// the creature's own name and the shared one). One that neither falls nor
/// has a dying clip is counted as standing when dead.
fn death_census(
    tally: &mut Tally,
    (module, template): (&str, &str),
    (look, body): (&creature::CreatureLook, &ModelIr),
    mut found: impl FnMut(&str, &str) -> bool,
) {
    let (humanoid, kind) = drawn(body);
    let example = || format!("{module}: {template} ({})", look.body);
    let falls = match kjk_game::doll_fits(body, humanoid, false) {
        Ok(()) => {
            tally.add(format!("death ({kind}): ragdoll"), example);
            true
        }
        Err(why) => {
            tally.add(format!("death ({kind}): no ragdoll, {why}"), example);
            false
        }
    };
    let die = found("cdie", "die");
    let dead = found("cdead", "dead");
    tally.add(
        format!("death ({kind}): dying clip {}", if die { "found" } else { "missing" }),
        example,
    );
    if !humanoid {
        tally.add(
            format!("death ({kind}): dead clip {}", if dead { "found" } else { "missing" }),
            example,
        );
    }
    // A humanoid lies by JA's deaths under any missing clip; a body on its
    // own clips needs one of them, where it dies in place, and in free play
    // too if it has no ragdoll.
    if !humanoid && !die && !dead {
        let when = if falls { "in place" } else { "always" };
        tally.add(format!("death ({kind}): stands when dead, {when}"), example);
    }
}

/// A humanoid body (retargeted) of a creature model type (S or L, a
/// gammorean's): which of the scripts' and the engine's clips
/// (`acting::clips`) its chain has under the creature name only (`c…`,
/// which KOTOR asks a creature model type for), under both, or under the
/// shared name only (`found`: a clip by name in its chain).
fn creature_names_census(
    tally: &mut Tally,
    (module, template): (&str, &str),
    look: &creature::CreatureLook,
    mut found: impl FnMut(&str) -> bool,
) {
    let example = || format!("{module}: {template} ({})", look.body);
    tally.add("humanoid of a creature model type", example);
    let names: std::collections::BTreeSet<&str> =
        crate::acting::clips().map(|(_, name)| name).collect();
    for name in names {
        let own = found(&format!("c{name}"));
        let shared = found(name);
        let which = match (own, shared) {
            (true, false) => "its creature name only",
            (true, true) => "both names",
            (false, true) => "the shared name only",
            (false, false) => continue,
        };
        tally.add(
            format!("humanoid of a creature model type: {name} under {which}"),
            example,
        );
    }
}

pub fn run(discovery: &Discovery, game: &str) -> ExitCode {
    let Some(game) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("clips: use k1 or tsl");
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
    let names = module_names(&index);
    // Every model a room, placeable or creature draws, once.
    let mut fountains_seen = BTreeSet::new();
    let routines = match crate::script_verbs::catalog(&base) {
        Ok(catalog) => animation_routines(&catalog),
        Err(error) => {
            eprintln!("clips: no routine catalog ({error}): scripts' animations not counted");
            Vec::new()
        }
    };
    for name in &names {
        let Ok(resolver) = base.with_module(&index, name) else {
            tally.add("module refused", || name.clone());
            continue;
        };
        let Ok(module) = ModuleIr::build(name, &ResolverSource(&resolver)) else {
            tally.add("module refused", || name.clone());
            continue;
        };
        let mut cache: BTreeMap<String, Option<ModelIr>> = BTreeMap::new();
        let mut tables = asset_odyssey::tables::Tables::default();
        let mut played: BTreeMap<String, BTreeSet<u16>> = BTreeMap::new();
        for area in &module.areas {
            for instance in area.instances.iter().filter(|i| i.kind == Kind::Creature) {
                let Ok(look) = creature::resolve_with(
                    &ResolverSource(&resolver),
                    &instance.template,
                    &mut tables,
                ) else {
                    tally.add("creature refused", || {
                        format!("{name}: {}", instance.template)
                    });
                    continue;
                };
                let body = match viewer::cached(&mut cache, &resolver, &look.body) {
                    Ok(Some(body)) => body,
                    _ => {
                        tally.add("body model missing", || format!("{name}: {}", look.body));
                        continue;
                    }
                };
                if fountains_seen.insert(look.body.clone()) {
                    fountains(&mut tally, &resolver, &look.body);
                }
                let creature_type = asset_odyssey::creature::creature_model(&look.model_type);
                let (humanoid, drawn) = drawn(&body);
                // The animations its own scripts play, and whether its body
                // has each one's clip.
                let mut animations = BTreeSet::new();
                for (_, script) in &instance.scripts {
                    let found = played
                        .entry(script.clone())
                        .or_insert_with(|| animations_played(&resolver, script, &routines));
                    animations.extend(found.iter().copied());
                }
                if !animations.is_empty() {
                    let acted = match viewer::scripted_clips(
                        &mut cache,
                        &resolver,
                        (game, creature_type),
                        &body,
                    ) {
                            Ok(acted) => acted,
                            Err(error) => {
                                tally.add("scripted clips refused", || error);
                                Vec::new()
                            }
                        };
                    for animation in animations {
                        let has = acted.iter().any(|(a, _)| *a == animation);
                        let found = match has {
                            true => "clip found".to_string(),
                            false => format!("no clip for {animation}"),
                        };
                        tally.add(
                            format!("scripted animation ({drawn}): {found}"),
                            || format!("{name}: {} {animation} ({})", instance.template, look.body),
                        );
                    }
                }
                if !body.lights.is_empty() {
                    tally.add(format!("body with lights ({drawn})"), || {
                        format!("{name}: {} ({})", instance.template, look.body)
                    });
                }
                for emitter in &body.emitters {
                    tally.add(
                        format!(
                            "body emitter, {:?} ({})",
                            emitter.update, drawn
                        ),
                        || format!("{} {}", look.body, emitter.name),
                    );
                }
                let wanted: &[(&str, &str)] = if creature_type {
                    &[("cpause1", "pause1"), ("cwalk", "walk"), ("crun", "run")]
                } else {
                    &[
                        ("pause1", "pause1"),
                        ("walk", "walk"),
                        ("run", "run"),
                        ("tlknorm", "tlknorm"),
                    ]
                };
                death_census(&mut tally, (name, &instance.template), (&look, &body), |c, s| {
                    matches!(
                        viewer::creature_clip(&mut cache, &resolver, game, &body, creature_type, c, s),
                        Ok(Some(_))
                    )
                });
                if humanoid && creature_type {
                    creature_names_census(&mut tally, (name, &instance.template), &look, |n| {
                        matches!(
                            viewer::creature_clip(&mut cache, &resolver, game, &body, false, n, n),
                            Ok(Some(_))
                        )
                    });
                }
                if body.animation_scale != 1.0 {
                    tally.add(
                        format!("body animation scale {}", body.animation_scale),
                        || look.body.clone(),
                    );
                }
                for &(clip_name, shared) in wanted {
                    let found = viewer::creature_clip(
                        &mut cache,
                        &resolver,
                        game,
                        &body,
                        creature_type,
                        clip_name,
                        shared,
                    );
                    let Ok(Some(clip)) = found else {
                        tally.add(
                            format!("{clip_name}: missing ({})", look.model_type),
                            || format!("{name}: {} ({})", instance.template, look.body),
                        );
                        continue;
                    };
                    let bound = clip
                        .tracks
                        .iter()
                        .filter(|track| {
                            body.nodes.iter().any(|node| {
                                node.animated && node.name.eq_ignore_ascii_case(&track.node)
                            })
                        })
                        .count();
                    tally.add(format!("{clip_name}: found ({})", look.model_type), || {
                        format!("{} via {}", look.body, clip.name)
                    });
                    // Position keys against the rest offsets they add to.
                    for track in &clip.tracks {
                        let Some(node) = body.nodes.iter().find(|node| {
                            node.animated && node.name.eq_ignore_ascii_case(&track.node)
                        }) else {
                            if clip_name.ends_with("pause1") {
                                tally.add(format!("unbound track ({})", look.model_type), || {
                                    track.node.to_ascii_lowercase()
                                });
                            }
                            continue;
                        };
                        if track.positions.is_empty() || !clip_name.ends_with("pause1") {
                            continue;
                        }
                        let length = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                        let key = track
                            .positions
                            .iter()
                            .map(|(_, v)| length(*v))
                            .fold(0.0, f32::max);
                        let rest = length(node.translation);
                        let bucket = if rest < 0.001 {
                            "rest at parent"
                        } else if key < 0.1 * rest {
                            "key < 10% of rest"
                        } else if key < rest {
                            "key < rest"
                        } else {
                            "key >= rest"
                        };
                        tally.add(format!("idle position track: {bucket}"), || {
                            format!("{}/{}: key {key:.3} rest {rest:.3}", look.body, track.node)
                        });
                    }
                    if bound < clip.tracks.len() {
                        tally.add(
                            format!("{clip_name}: some tracks bind to no animated node"),
                            || format!("{}: {bound} of {}", look.body, clip.tracks.len()),
                        );
                    }
                }
            }
            let drawn = (area.rooms.iter().map(|room| room.model.to_ascii_lowercase()))
                .chain(area.instances.iter().filter_map(|i| i.model.clone()));
            for model in drawn {
                if fountains_seen.insert(model.clone()) {
                    fountains(&mut tally, &resolver, &model);
                }
            }
            for instance in area.instances.iter().filter(|i| i.kind == Kind::Placeable) {
                placeable_look(
                    &mut tally,
                    (&mut cache, &resolver, game),
                    name,
                    instance,
                );
            }
        }
    }
    outln!("{}: {} modules", game.key(), names.len());
    for (key, count) in &tally.counts {
        outln!("  {key}: {count} (e.g. {})", tally.examples[key].join("; "));
    }
    ExitCode::SUCCESS
}

/// A placeable's look as placed (`sim::look::placed` of its
/// `AnimationState`) and where its model finds that look's clip: its own,
/// through its supermodel chain, or nowhere, a skinned model then standing
/// in its bind pose.
fn placeable_look(
    tally: &mut Tally,
    (cache, resolver, game): (&mut BTreeMap<String, Option<ModelIr>>, &Resolver, Game),
    module: &str,
    instance: &asset_odyssey::module::Instance,
) {
    let Some(model) = &instance.model else {
        return;
    };
    let Ok(Some(ir)) = viewer::cached(cache, resolver, model) else {
        return;
    };
    let wanted = sim::look::clip_name(sim::look::placed(instance.animation_state));
    let skinned = ir.meshes.iter().any(|mesh| mesh.skin.is_some());
    let kind = if skinned { "skinned" } else { "rigid" };
    let own = ir.animations.iter().any(|clip| clip.name == wanted);
    let chain = !own && matches!(viewer::clip(cache, resolver, game, &ir, &wanted), Ok(Some(_)));
    let found = match (own, chain) {
        (true, _) => "own",
        (false, true) => "through its supermodel",
        _ if ir.animations.is_empty()
            && ir
                .supermodel
                .as_deref()
                .is_none_or(|name| name.trim().is_empty() || name.eq_ignore_ascii_case("null")) =>
        {
            return;
        }
        _ => "missing",
    };
    tally.add(
        format!("placeable look {wanted}: {found} ({kind})"),
        || format!("{module}: {} ({model})", instance.template),
    );
}

/// The routines that play an animation on an object (`ActionPlayAnimation`,
/// `PlayAnimation`), by number in the game's `nwscript.nss`.
fn animation_routines(catalog: &script_ncs::Catalog) -> Vec<u16> {
    (catalog.routines.iter().enumerate())
        .filter(|(_, routine)| {
            routine.name == "ActionPlayAnimation" || routine.name == "PlayAnimation"
        })
        .filter_map(|(number, _)| u16::try_from(number).ok())
        .collect()
}

/// The `ANIMATION_…` constants compiled script `script` passes as they are
/// to the animation routines (a literal pushed last, the first argument).
fn animations_played(resolver: &Resolver, script: &str, routines: &[u16]) -> BTreeSet<u16> {
    use fmt_odyssey::ncs::{Const, Ncs, Op};
    let Some(found) = resolver.find(script, fmt_odyssey::ResType::NCS) else {
        return BTreeSet::new();
    };
    let Some(ncs) = found.location.read().ok().and_then(|bytes| Ncs::parse(&bytes).ok()) else {
        return BTreeSet::new();
    };
    ncs.code
        .windows(2)
        .filter_map(|pair| match (&pair[0].op, &pair[1].op) {
            (Op::Const(Const::Int(animation)), Op::Action { routine, .. })
                if routines.contains(routine) =>
            {
                u16::try_from(*animation).ok()
            }
            _ => None,
        })
        .collect()
}

/// How model `name`'s fountains (a room's, a placeable's or, through its
/// supermodels' clips, a body's) bear (`model_fx::Particles`): at rest, or
/// not (a birth rate under 1, with or without a random birth rate), and
/// whether a clip keys them to.
fn fountains(tally: &mut Tally, resolver: &Resolver, name: &str) {
    let Ok(Some(parsed)) = viewer::parsed(resolver, name) else {
        return;
    };
    let fx = viewer::fx_through_chain(resolver, &parsed);
    let fountains = (fx.emitters.iter()).filter(|e| e.update == kjk_game::model_fx::Update::Fountain);
    for emitter in fountains {
        let (rate, random, keyed) = emitter.birth_rates();
        let at_rest = match (rate >= 1.0, random > 0.0) {
            (true, _) => "bears at rest",
            (false, true) => "silent at rest, a random birth rate alone",
            (false, false) => "silent at rest",
        };
        let clips = match keyed {
            Some(most) if most >= 1.0 => "; a clip keys it to bear",
            Some(_) => "; its clips key it under 1",
            None => "",
        };
        tally.add(format!("fountain: {at_rest}{clips}"), || {
            format!("{name} {} ({rate}+{random})", emitter.node)
        });
    }
}
