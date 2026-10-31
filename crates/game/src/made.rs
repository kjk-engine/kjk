//! The creatures made in play, drawn as a module's own are: those
//! `SpawnAvailableNPC` and `CreateObject` make, and the party's companions
//! carried into the next module (docs/WORLD-STATE.md, "Made in play"). One
//! is a creature instance play made (`Instances::made_from`: the module's
//! own came with its scene), of the template it was made from.
//!
//! The first presented snapshot that has one starts loading it off the
//! frame, on the task pool, as `viewer::module_scene` loads a module's
//! creature (`CreatureLoad::place`: its look, body, head, weapons and
//! clips, and their textures). A later frame finds the load done and spawns
//! it under the scene's root. Its models and view join the scene
//! (`SceneContent`, changed unnoticed so the scene is not spawned again), so
//! the views find it as they find a module's creature (`cast`, `combat_fx`,
//! a speaker's head in `dialogue_view`); its weapons go to `play_view`
//! (`Made::arrived`) to be held. When the snapshot no longer draws it
//! (destroyed and done departing, `sim::Departing`, or left in the module
//! the party left), its view leaves the
//! scene and `play_view` forgets its character and despawns its models
//! (`Made::left`); one still loading is dropped.
//!
//! A made placeable (`CreateObject`'s: a party's backpack) is drawn the
//! same way, simpler: its template's model (`placeables.2da` by its
//! appearance; the dead's remains by their `bodybag.2da` row's,
//! `sim::remains`) where it was made, turned to its facing
//! (`Spawned::yaw`); when the snapshot no longer draws it, its models leave
//! the scene and are despawned here.
//!
//! An item lying on the area's floor (`CreateItemOnFloor`: the snapshot's
//! `Goods::floor`) is drawn the same way: its base item's model at its
//! `ModelVariation` where it lies, turned as `rotateonground` lays it
//! (`asset_odyssey::item::ground_turn`), then to its facing; taken or
//! destroyed, it leaves.
//!
//! A trap the party detected (`sim::snapshot::trap::PARTY_SEES`), placed or
//! laid, is drawn the same way: its `traps.2da` row's model at the trap's
//! middle, posed as its `detect` clip ends (`viewer::held`: the dome over
//! its trigger raised and lit, the model's own red for a trap hostile to
//! the party, green for the party's own). The dome is additive, which the
//! Odyssey material draws unlit, its self-illumination unused; the games
//! key that colour on it (red detected, blue disarmed), so a mine's meshes
//! are tinted by their own self-illumination once they stand
//! (`tint_mines`). The stealth field's
//! texture (`VFX_DUR_STEALTH_FIELD`'s glow, `script_fx::lasting_glow`) is
//! read here once for the stealth shell (`stealth_view`).
//!
//! A character wearing a disguise (an item's Disguise property: its
//! sheet's `Gear::disguise`, an `appearance.2da` row) is drawn as that
//! appearance while it wears it; a creature (a companion, any other) whose
//! body armour is no longer what its template put on it is drawn dressed
//! again, as KOTOR rebuilds a creature's body when its armour changes
//! (`guise`; the players' own bodies are `wardrobe`'s). The row's body (by
//! the item in its body slot, as a template's look is dressed) is read off
//! the frame as a made
//! creature's is (`CreatureLoad::place_appearance`), spawned hidden under
//! the scene's root, and the cast draws its wearer in it until it takes the
//! item off (`cast::Cast::disguise`). Each wearer's body is kept for the
//! scene, so putting it on again is a swap; a character the snapshot no
//! longer has takes its disguises' bodies with it.
//!
//! Cheap per frame: a made creature is loaded once, off the frame (a
//! companion carried into a new module, its archives opened for it, takes
//! up to half a second from a cold disk), and only spawned in one. What
//! loading reads stays for the scene (the module's resolver, opened once;
//! the looks, models and clips, `CreatureLoad`), so another of the same
//! template reads only its textures again.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use asset_odyssey::texture::TextureIr;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use console::{DumpDue, DumpFacts};
use sim::module::ObjectKind;
use sim::{EntityState, Instances, Kind, SimId, Snapshot, Spawned};
use vfs::Game;
use vfs::resolver::Resolver;

use crate::lifecycle::Installer;
use crate::scene::{LiveBlades, ModelAssets, SceneContent, SceneRoot, Spawner};
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::viewer::{self, BladeSpec, CreatureLoad, CreatureView, LoadedScene, Placed};
use crate::weapons::{self, Armed, Arms, Sabers};

/// What made creatures are loaded with.
pub struct Setup {
    /// The install's modules, the templates and models come from (`None`:
    /// a stage without one draws none).
    pub installer: Option<Arc<Installer>>,
    /// The JA sabers their lightsabers' blades take after, and the
    /// blades' shaders' textures (every colour's, as the armory's).
    pub sabers: Sabers,
    pub blade_textures: BTreeMap<String, TextureIr>,
}

/// A made creature come into the scene: its weapons, spawned, for its
/// character to hold when it is drawn.
pub struct Arrival {
    pub index: u32,
    pub id: SimId,
    pub arms: [Option<Armed>; 2],
}

/// A made creature gone: its character, and its body's root (its head and
/// weapons hang under it).
pub struct Departure {
    pub index: u32,
    pub id: SimId,
    pub root: Entity,
}

/// What a disguised character's body is: the `appearance.2da` row its
/// disguise makes it, the item in its body slot (a row of model type `B`
/// dresses its body by it), and the head a script gave it (a `heads.2da`
/// row; else the row's own).
pub type Guise = (u16, Option<String>, Option<u8>);

/// A disguise's body the scene has: its root and models, and how the views
/// animate it.
struct Guised {
    root: Entity,
    models: Range<usize>,
    view: CreatureView,
}

/// A made creature the scene has: its template, its character as last
/// seen, and its models (`None` when it could not be loaded: it is not
/// tried again).
struct Drawn {
    template: String,
    id: SimId,
    models: Option<(Entity, Range<usize>)>,
}

/// A module made creatures are read from: its resolver (opened by the
/// first load that needs it, the other loads not waiting on the map) and
/// what they read so far.
#[derive(Default)]
struct Module {
    resolver: OnceLock<Result<Resolver, String>>,
    load: Mutex<CreatureLoad>,
}

/// The modules opened for the scene's made creatures, by name (shared with
/// the loads' threads).
type Modules = Arc<Mutex<BTreeMap<String, Arc<Module>>>>;

/// A made creature read and readied off the frame: its models (their
/// indices, the view's and the hooks', count among themselves), how the
/// views animate it, its weapons and their blades, its textures.
struct Ready {
    models: Vec<Placed>,
    view: CreatureView,
    arms: Arms,
    blades: Vec<BladeSpec>,
    textures: BTreeMap<String, TextureIr>,
    notes: Vec<String>,
    ms: f64,
}

/// A made placeable read off the frame: its model placed and turned, its
/// textures, and what its own clips and emitters do (`own_fx`; its model
/// then the scene's model it is, filled as it is spawned).
struct Prop {
    models: Vec<Placed>,
    textures: BTreeMap<String, TextureIr>,
    own: Option<viewer::OwnFx>,
}

/// A made placeable the scene has: its template, and its model's root and
/// indices (`None`: not drawn; not tried again).
struct PropDrawn {
    template: String,
    models: Option<(Entity, Range<usize>)>,
}

/// A made creature being loaded: of which template, since which tick and
/// when.
struct Loading {
    template: String,
    tick: u64,
    since: Instant,
    task: Task<Result<Ready, String>>,
}

#[derive(Resource)]
pub struct Made {
    setup: Setup,
    /// The scene swap the drawn creatures belong to.
    generation: u32,
    /// The modules the session has been in, the latest last: a companion
    /// carried on from one whose own template it is (a module's creatures
    /// are in its archives only) is read from there.
    visited: Vec<String>,
    modules: Modules,
    /// By object index: those loading, those the scene has.
    loading: BTreeMap<u32, Loading>,
    drawn: BTreeMap<u32, Drawn>,
    /// Made placeables, by object index: loading (with their template),
    /// and those the scene has.
    props_loading: BTreeMap<u32, (String, Task<Result<Prop, String>>)>,
    props: BTreeMap<u32, PropDrawn>,
    /// Detected traps' models, by object index: loading (with their model's
    /// name and tint), and those the scene has.
    mines_loading: BTreeMap<u32, (String, Task<Result<Prop, String>>)>,
    mines: BTreeMap<u32, PropDrawn>,
    /// Items lying on the floor, by item id: loading (with their
    /// template), and those the scene has.
    floor_loading: BTreeMap<u32, (String, Task<Result<Prop, String>>)>,
    floor: BTreeMap<u32, PropDrawn>,
    /// Disguises, by character: the guise each wears in the presented
    /// snapshot, the bodies loading (with their guise), and those the scene
    /// has by character and guise (`None`: not drawn; not tried again).
    worn: BTreeMap<SimId, Guise>,
    guises_loading: BTreeMap<SimId, (Guise, Task<Result<Ready, String>>)>,
    guises: BTreeMap<(SimId, Guise), Option<Guised>>,
    /// The stealth field's texture, once read (`stealth_view`).
    pub stealth_texture: Option<Handle<Image>>,
    stealth_loading: Option<Task<Option<TextureIr>>>,
    /// For `play_view`, since its last frame.
    pub arrived: Vec<Arrival>,
    pub left: Vec<Departure>,
    loads: usize,
    /// Milliseconds, the last load's: reading it (off the frame) and
    /// spawning it (in one).
    last_ms: f64,
    last_spawn_ms: f64,
    notes: Vec<String>,
}

/// The template of creature `index` when play made it.
pub fn template(objects: &Instances, index: u32) -> Option<&str> {
    let made = objects.made_from(index)?;
    (made.kind == ObjectKind::Creature).then_some(made.template.as_str())
}

impl Made {
    pub fn new(setup: Setup) -> Made {
        Made {
            setup,
            generation: 0,
            visited: Vec::new(),
            modules: Modules::default(),
            loading: BTreeMap::new(),
            drawn: BTreeMap::new(),
            props_loading: BTreeMap::new(),
            props: BTreeMap::new(),
            mines_loading: BTreeMap::new(),
            mines: BTreeMap::new(),
            floor_loading: BTreeMap::new(),
            floor: BTreeMap::new(),
            worn: BTreeMap::new(),
            guises_loading: BTreeMap::new(),
            guises: BTreeMap::new(),
            stealth_texture: None,
            stealth_loading: None,
            arrived: Vec::new(),
            left: Vec::new(),
            loads: 0,
            last_ms: 0.0,
            last_spawn_ms: 0.0,
            notes: Vec::new(),
        }
    }

    /// Made placeable `index`'s model root, once it is spawned.
    pub fn prop_root(&self, index: u32) -> Option<Entity> {
        Some(self.props.get(&index)?.models.as_ref()?.0)
    }

    /// Whether creature `index` is one made in play (loading or drawn):
    /// its leaving is this module's own.
    pub fn makes(&self, index: u32) -> bool {
        self.loading.contains_key(&index) || self.drawn.contains_key(&index)
    }

    /// The body character `id` is drawn in while it wears a disguise, once
    /// the scene has it: how the views animate it (its models the scene's).
    pub fn disguise(&self, id: SimId) -> Option<&CreatureView> {
        let guise = self.worn.get(&id)?.clone();
        Some(&self.guises.get(&(id, guise))?.as_ref()?.view)
    }

    /// Starts reading creature `index` (`entity`, made from `template`) on
    /// the task pool, from the scene's module (`level`'s) when it has the
    /// template, else the latest one before that has it (each one's
    /// resolver opened the first time it is asked).
    fn load(
        &self,
        (index, entity, template): (u32, &EntityState, &str),
        level: &sim::Level,
    ) -> Task<Result<Ready, String>> {
        // Where it stands and faces (game space, metres; the view places
        // it every frame from then on).
        let at = entity.player.map_or(([0.0; 3], 0.0), |player| {
            let soles = crate::cast::soles(player.origin).to_array();
            (soles, player.view[1].to_radians())
        });
        let (has, template) = (template.to_string(), template.to_string());
        self.read(
            level,
            template.clone(),
            move |load, resolver| load.has(resolver, &has),
            move |load, source, models| load.place(source, (index, &template), at, models),
        )
    }

    /// Starts reading the body of `guise` on the task pool, from the
    /// scene's module (`appearance.2da` and the models are the game's).
    fn load_guise(
        &self,
        (row, armour, head): Guise,
        level: &sim::Level,
    ) -> Task<Result<Ready, String>> {
        self.read(
            level,
            format!("appearance {row}"),
            |_, _| true,
            move |load, source, models| {
                let at = ([0.0; 3], 0.0);
                let body = (u32::from(row), armour.as_deref(), head);
                load.place_appearance(source, body, at, models)
            },
        )
    }

    /// Starts reading creature `name` on the task pool, from the first of
    /// the scene's module and those before it that `has` it, as `place`
    /// places it there: its models, weapons, their blades and textures.
    fn read(
        &self,
        level: &sim::Level,
        name: String,
        has: impl Fn(&mut CreatureLoad, &Resolver) -> bool + Send + 'static,
        place: impl FnOnce(
            &mut CreatureLoad,
            (&Resolver, Game),
            &mut Vec<Placed>,
        ) -> Result<Option<CreatureView>, String>
        + Send
        + 'static,
    ) -> Task<Result<Ready, String>> {
        let installer = self.setup.installer.clone();
        let modules = self.modules.clone();
        let sabers = self.setup.sabers.clone();
        let blade_textures = self.setup.blade_textures.clone();
        let names = self.names(level);
        AsyncComputeTaskPool::get().spawn(async move {
            let started = Instant::now();
            let installer = installer.ok_or("no install to make creatures from")?;
            let module = module_with(&installer, &modules, &names, |module, resolver| {
                let mut load = module.load.lock().map_err(|_| "a module's load lock")?;
                Ok(has(&mut load, resolver))
            })?;
            let resolver = module
                .resolver
                .get()
                .ok_or("no resolver")?
                .as_ref()
                .map_err(Clone::clone)?;
            let mut models = Vec::new();
            let view = place(
                &mut *module.load.lock().map_err(|_| "a module's load lock")?,
                (resolver, installer.game),
                &mut models,
            )?
            .ok_or(format!("{name}: not drawn (see the refusals)"))?;
            let (mut blades, mut notes) = (Vec::new(), Vec::new());
            let arms = weapons::arm(&mut models, &mut blades, view.weapons, &sabers, &mut notes);
            let (mut textures, missing) = viewer::textures(resolver, &models);
            if !blades.is_empty() {
                textures.extend(blade_textures);
            }
            if let Some(fx) = &view.fx {
                crate::model_fx::add_textures(resolver, (&fx.fx, &fx.chunks), &mut textures);
            }
            notes.extend(
                missing
                    .iter()
                    .map(|texture| format!("{name}: no texture {texture}")),
            );
            Ok(Ready {
                models,
                view,
                arms,
                blades,
                textures,
                notes,
                ms: started.elapsed().as_secs_f64() * 1000.0,
            })
        })
    }

    /// Starts reading placeable `template`, object `index`, made at `at`
    /// (game space, metres) facing `yaw` (degrees), on the task pool, from
    /// the first of the scene's module and those before it that has its
    /// template (any, for remains: their appearance is their bag row's).
    fn load_prop(
        &self,
        (index, template, at, yaw): (u32, &str, [f32; 3], f32),
        level: &sim::Level,
    ) -> Task<Result<Prop, String>> {
        let installer = self.setup.installer.clone();
        let modules = self.modules.clone();
        let names = self.names(level);
        let template = template.to_string();
        let bag = level
            .module()
            .and_then(|module| sim::remains::bag_of(module, &template))
            .map(|row| row.placeable());
        AsyncComputeTaskPool::get().spawn(async move {
            let installer = installer.ok_or("no install to make placeables from")?;
            let module = module_with(&installer, &modules, &names, |_, resolver| {
                Ok(bag.is_some()
                    || resolver
                        .find(&template, fmt_odyssey::ResType::UTP)
                        .is_some())
            })?;
            let resolver = module
                .resolver
                .get()
                .ok_or("no resolver")?
                .as_ref()
                .map_err(Clone::clone)?;
            let (model, placed) = match bag {
                Some(appearance) => (
                    appearance_model(resolver, usize::from(appearance))?,
                    sim::look::DEFAULT,
                ),
                None => prop_model(resolver, &template)?,
            };
            let parsed =
                viewer::parsed(resolver, &model)?.ok_or(format!("{template}: no model {model}"))?;
            let ir = asset_odyssey::model::ModelIr::from_model(&parsed);
            let own = viewer::placeable_fx(
                (&mut BTreeMap::new(), resolver, installer.game),
                &parsed,
                &ir,
                (index, placed),
            );
            let models = vec![Placed {
                facing: yaw.to_radians(),
                ..Placed::at(ir, at)
            }];
            let (mut textures, _) = viewer::textures(resolver, &models);
            if let Some(own) = &own {
                crate::model_fx::add_textures(resolver, (&own.fx, &own.chunks), &mut textures);
            }
            Ok(Prop {
                models,
                textures,
                own,
            })
        })
    }

    /// Starts reading a detected trap's model `model`, placed at `at`
    /// (game space, metres) and posed as its `detect` clip ends, its dome
    /// green when `friendly`, on the task pool, from the first of the
    /// scene's module and those before it that has it.
    fn load_mine(
        &self,
        (model, at, friendly): (&str, [f32; 3], bool),
        level: &sim::Level,
    ) -> Task<Result<Prop, String>> {
        let installer = self.setup.installer.clone();
        let modules = self.modules.clone();
        let names = self.names(level);
        let model = model.to_string();
        AsyncComputeTaskPool::get().spawn(async move {
            let installer = installer.ok_or("no install to draw mines from")?;
            let module = module_with(&installer, &modules, &names, |_, resolver| {
                Ok(resolver.find(&model, fmt_odyssey::ResType::MDL).is_some())
            })?;
            let resolver = module
                .resolver
                .get()
                .ok_or("no resolver")?
                .as_ref()
                .map_err(Clone::clone)?;
            let mut parsed =
                viewer::parsed(resolver, &model)?.ok_or(format!("no model {model}"))?;
            viewer::held(&mut parsed, "detect", friendly.then_some(FRIENDLY));
            let ir = asset_odyssey::model::ModelIr::from_model(&parsed);
            let models = vec![Placed::at(ir, at)];
            let (textures, _) = viewer::textures(resolver, &models);
            Ok(Prop {
                models,
                textures,
                own: None,
            })
        })
    }

    /// Starts reading item `template` lying on the floor as `lying` says
    /// (game space, metres; its facing in degrees) on the task pool, from the
    /// first of the scene's module and those before it that has its
    /// template: its model (`floor_model`) turned as its base item lies
    /// (`item::ground_turn`), then to its facing.
    fn load_floor_item(
        &self,
        (template, lying): (&str, sim::inventory::Lying),
        level: &sim::Level,
    ) -> Task<Result<Prop, String>> {
        let installer = self.setup.installer.clone();
        let modules = self.modules.clone();
        let names = self.names(level);
        let template = template.to_string();
        AsyncComputeTaskPool::get().spawn(async move {
            let installer = installer.ok_or("no install to draw items from")?;
            let module = module_with(&installer, &modules, &names, |_, resolver| {
                Ok(resolver
                    .find(&template, fmt_odyssey::ResType::UTI)
                    .is_some())
            })?;
            let resolver = module
                .resolver
                .get()
                .ok_or("no resolver")?
                .as_ref()
                .map_err(Clone::clone)?;
            let (model, turn) =
                asset_odyssey::creature::floor_model(&viewer::ResolverSource(resolver), &template)
                    .ok_or(format!("{template}: no item model"))?;
            let parsed =
                viewer::parsed(resolver, &model)?.ok_or(format!("{template}: no model {model}"))?;
            let ir = asset_odyssey::model::ModelIr::from_model(&parsed);
            let models = vec![Placed {
                facing: lying.yaw.to_radians(),
                attach: Some(asset_odyssey::item::ground_turn(turn)),
                ..Placed::at(ir, lying.at)
            }];
            let (textures, _) = viewer::textures(resolver, &models);
            Ok(Prop {
                models,
                textures,
                own: None,
            })
        })
    }

    /// Starts reading the stealth field's texture, once.
    fn load_stealth_texture(&mut self, level: &sim::Level) {
        if self.stealth_texture.is_some() || self.stealth_loading.is_some() {
            return;
        }
        let Some(installer) = self.setup.installer.clone() else {
            return;
        };
        let modules = self.modules.clone();
        let names = self.names(level);
        self.stealth_loading = Some(AsyncComputeTaskPool::get().spawn(async move {
            let module = module_with(&installer, &modules, &names, |_, _| Ok(true)).ok()?;
            let resolver = module.resolver.get()?.as_ref().ok()?;
            let glow = crate::script_fx::lasting_glow(resolver, VFX_DUR_STEALTH_FIELD)?;
            viewer::texture(resolver, &glow)
        }));
    }

    /// The modules a made object's template is looked for in: the scene's,
    /// then those before it, the latest first; then the module whose data
    /// the level runs on, where it is named otherwise (the arena's).
    fn names(&self, level: &sim::Level) -> Vec<String> {
        let data = level
            .module()
            .map(|module| module.name.as_str())
            .filter(|name| *name != level.name());
        std::iter::once(level.name())
            .chain(self.visited.iter().rev().map(String::as_str))
            .chain(data)
            .map(str::to_string)
            .collect()
    }

    /// Placeable `prop`, read, into `scene` and spawned under `root`: its
    /// model's root and its models' indices. Its meshes whose UVs scroll
    /// (`animateuv`) join the scene's (`scrolling`), its own clips and
    /// emitters the scene's `own_fx`.
    fn spawn_prop(
        prop: Prop,
        (scene, root): (&mut LoadedScene, Entity),
        (assets, commands, scrolling): (
            &mut ModelAssets,
            &mut Commands,
            &mut crate::scene::ScrollingMaterials,
        ),
    ) -> (Entity, Range<usize>) {
        let first = scene.models.len();
        scene.models.extend(prop.models);
        if let Some(own) = prop.own {
            scene.own_fx.push(viewer::OwnFx {
                model: first,
                ..own
            });
        }
        let (images, mut stores) = assets.split();
        let mut spawner = Spawner::new(&prop.textures, scene.area_light(), images);
        let (mut roots, mut nodes) = (Vec::new(), Vec::new());
        for (n, placed) in scene.models[first..].iter().enumerate() {
            let (model, its) = spawner.placed(
                commands,
                &mut stores,
                (placed, first + n, !scene.lit),
                (root, &nodes, first),
            );
            roots.push(model);
            nodes.push(its);
        }
        scrolling.0.append(&mut spawner.scrolling);
        drop(spawner);
        for (name, texture) in prop.textures {
            scene.textures.entry(name).or_insert(texture);
        }
        (roots[0], first..scene.models.len())
    }

    /// Placeable `index` is gone: its models leave `scene` and are
    /// despawned.
    fn depart_prop(&mut self, index: u32, scene: &mut LoadedScene, commands: &mut Commands) {
        leave(self.props.remove(&index), scene, commands);
    }

    /// Creature `index` (`entity`), read (`ready`), into `scene` and
    /// spawned under `root`: its body's root and its models' indices, its
    /// weapons handed on (`arrived`).
    fn spawn(
        &mut self,
        (index, entity, mut ready): (u32, &EntityState, Ready),
        (scene, root): (&mut LoadedScene, Entity),
        (lives, assets, commands): (&mut LiveBlades, &mut ModelAssets, &mut Commands),
    ) -> (Entity, Range<usize>) {
        self.notes.append(&mut ready.notes);
        let (view, arms, body, models) = spawned(ready, (scene, root), (lives, assets, commands));
        scene.creature_views.push(view);
        self.arrived.push(Arrival {
            index,
            id: entity.id,
            arms,
        });
        (body, models)
    }

    /// Creature `index` is gone: its view leaves `scene` (its models'
    /// meshes freed; their places stay, as other models are named by index)
    /// and `play_view` despawns it.
    fn depart(&mut self, index: u32, scene: &mut LoadedScene) {
        let Some(drawn) = self.drawn.remove(&index) else {
            return;
        };
        scene.creature_views.retain(|view| view.object != index);
        if let Some((root, models)) = drawn.models {
            for placed in &mut scene.models[models] {
                placed.ir.meshes = Vec::new();
                placed.clip = None;
            }
            self.left.push(Departure {
                index,
                id: drawn.id,
                root,
            });
        }
    }
}

/// Each frame, before `play_view` draws: made creatures the presented
/// snapshot has that the scene does not start loading, those loaded are
/// spawned, and those it no longer has go (see the module documentation).
/// Their facts, for a dump only.
#[allow(clippy::too_many_arguments)]
pub fn track(
    mut made: ResMut<Made>,
    presented: Res<PresentedSnapshot>,
    (swap, mut content): (Res<SceneSwap>, ResMut<SceneContent>),
    roots: Query<Entity, With<SceneRoot>>,
    mut lives: ResMut<LiveBlades>,
    mut assets: ModelAssets,
    mut commands: Commands,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
    (mut tints, mut scrolling): (ResMut<MineTints>, ResMut<crate::scene::ScrollingMaterials>),
) {
    let made = &mut *made;
    if swap.generation != made.generation {
        made.generation = swap.generation;
        if swap.scene {
            // Their models went with the old scene, and what was read for
            // them; loads under way are dropped.
            made.drawn.clear();
            made.loading.clear();
            made.props.clear();
            made.props_loading.clear();
            made.mines.clear();
            made.mines_loading.clear();
            made.floor.clear();
            made.floor_loading.clear();
            made.worn.clear();
            made.guises.clear();
            made.guises_loading.clear();
            made.modules = Modules::default();
            made.arrived.clear();
            made.left.clear();
        }
    }
    let snapshot = presented.0.current();
    let level = &swap.level;
    if made.visited.last().is_none_or(|name| name != level.name()) {
        made.visited.retain(|name| name != level.name());
        made.visited.push(level.name().to_string());
    }
    let objects = Instances::in_snapshot(level, snapshot);
    let present: BTreeMap<u32, (&EntityState, &str)> = snapshot
        .all_drawn()
        .filter_map(|entity| {
            let Kind::Object(ObjectKind::Creature, index) = entity.kind else {
                return None;
            };
            Some((index, (entity, template(&objects, index)?)))
        })
        .collect();
    // Noted by the views as they read it, never spawned again for it.
    let scene = &mut *content.bypass_change_detection().0;
    let gone: Vec<u32> = made
        .drawn
        .iter()
        .filter(|(index, drawn)| {
            present
                .get(index)
                .is_none_or(|(_, template)| !template.eq_ignore_ascii_case(&drawn.template))
        })
        .map(|(&index, _)| index)
        .collect();
    for index in gone {
        made.depart(index, scene);
    }
    made.loading.retain(|index, loading| {
        present
            .get(index)
            .is_some_and(|(_, template)| template.eq_ignore_ascii_case(&loading.template))
    });
    for (&index, &(entity, template)) in &present {
        if let Some(drawn) = made.drawn.get_mut(&index) {
            drawn.id = entity.id;
        } else if !made.loading.contains_key(&index) {
            let task = made.load((index, entity, template), level);
            let loading = Loading {
                template: template.to_string(),
                tick: snapshot.tick,
                since: Instant::now(),
                task,
            };
            made.loading.insert(index, loading);
        }
    }
    // Those read spawn once the scene has its root.
    let root = roots.iter().next();
    let done: Vec<(u32, Entity)> = made
        .loading
        .iter()
        .filter(|(_, loading)| loading.task.is_finished())
        .filter_map(|(&index, _)| Some((index, root?)))
        .collect();
    for (index, root) in done {
        let (Some(loading), Some(&(entity, _))) =
            (made.loading.remove(&index), present.get(&index))
        else {
            continue;
        };
        let Some(read) = block_on(poll_once(loading.task)) else {
            continue;
        };
        let started = Instant::now();
        let (models, read_ms) = match read {
            Ok(ready) => {
                let ms = ready.ms;
                let models = made.spawn(
                    (index, entity, ready),
                    (scene, root),
                    (&mut lives, &mut assets, &mut commands),
                );
                (Some(models), ms)
            }
            Err(error) => {
                made.notes.push(format!("#{index} {error}"));
                (None, 0.0)
            }
        };
        made.loads += 1;
        made.last_ms = read_ms;
        made.last_spawn_ms = started.elapsed().as_secs_f64() * 1000.0;
        info!(
            "made creature #{index} ({}, tick {}): {}, read in {read_ms:.1} ms off the frame, \
             spawned {:.0} ms after it came, in {:.1} ms",
            loading.template,
            loading.tick,
            if models.is_some() {
                "drawn"
            } else {
                "not drawn"
            },
            loading.since.elapsed().as_secs_f64() * 1000.0,
            made.last_spawn_ms
        );
        made.drawn.insert(
            index,
            Drawn {
                template: loading.template,
                id: entity.id,
                models,
            },
        );
    }
    if let Some(root) = root {
        track_props(
            made,
            (snapshot, &objects, level),
            (scene, root),
            (&mut assets, &mut commands, &mut scrolling),
        );
        track_mines(
            made,
            (snapshot, &objects, level),
            (scene, root),
            (&mut assets, &mut commands, &mut scrolling),
            &mut tints,
        );
        track_floor(
            made,
            (snapshot, level),
            (scene, root),
            (&mut assets, &mut commands, &mut scrolling),
        );
        track_disguises(
            made,
            (snapshot, &objects, level),
            (scene, root),
            (&mut lives, &mut assets, &mut commands),
        );
    }
    made.load_stealth_texture(level);
    if let Some(task) = made.stealth_loading.as_mut()
        && task.is_finished()
    {
        let read = made
            .stealth_loading
            .take()
            .and_then(|task| block_on(poll_once(task)))
            .flatten();
        match read {
            Some(texture) => {
                let (images, _) = assets.split();
                made.stealth_texture = Some(images.add(crate::scene::image(&texture)));
            }
            None => made.notes.push("no stealth field glow".into()),
        }
    }
    if !dump_due.0 {
        return;
    }
    facts.0.insert(
        "made".into(),
        serde_json::json!({
            "drawn": made.drawn.iter().map(|(index, drawn)| serde_json::json!({
                "index": index,
                "template": drawn.template,
                "id": drawn.id.0,
                "models": drawn.models.as_ref().map(|(_, models)| models.len()),
            })).collect::<Vec<_>>(),
            "loading": made.loading.keys().collect::<Vec<_>>(),
            "props": made.props.iter().map(|(index, drawn)| serde_json::json!({
                "index": index,
                "template": drawn.template,
                "drawn": drawn.models.is_some(),
            })).collect::<Vec<_>>(),
            "floor": made.floor.iter().map(|(id, drawn)| serde_json::json!({
                "item": id,
                "template": drawn.template,
                "drawn": drawn.models.is_some(),
            })).collect::<Vec<_>>(),
            "disguises": made.worn.iter().map(|(id, (row, armour, head))| serde_json::json!({
                "id": id.0,
                "appearance": row,
                "armour": armour,
                "head": head,
                "drawn": made.disguise(*id).is_some(),
                "loading": made.guises_loading.contains_key(id),
            })).collect::<Vec<_>>(),
            "loads": made.loads,
            "last_load_ms": made.last_ms,
            "last_spawn_ms": made.last_spawn_ms,
            "visited": made.visited,
            "modules": read(&made.modules),
            "notes": made.notes,
        }),
    );
}

/// A creature read (`ready`) into `scene` and spawned under `root`: how the
/// views animate it and its weapons (their indices now the scene's), its
/// body's root and its models' indices.
fn spawned(
    ready: Ready,
    (scene, root): (&mut LoadedScene, Entity),
    (lives, assets, commands): (&mut LiveBlades, &mut ModelAssets, &mut Commands),
) -> (CreatureView, [Option<Armed>; 2], Entity, Range<usize>) {
    let Ready {
        mut models,
        mut view,
        arms,
        blades,
        textures,
        ..
    } = ready;
    // Its indices from among its own models to the scene's.
    let first = scene.models.len();
    for placed in &mut models {
        if let Some((body, _)) = &mut placed.hook {
            *body += first;
        }
    }
    view.body += first;
    for weapon in view.weapons.iter_mut().flatten() {
        *weapon += first;
    }
    if let Some(fx) = &mut view.fx {
        fx.model = view.body;
        scene.own_fx.push(fx.clone());
    }
    scene.models.extend(models);
    let added = &scene.models[first..];
    let (images, mut stores) = assets.split();
    let mut spawner = Spawner::new(&textures, scene.area_light(), images);
    let (mut roots, mut nodes) = (Vec::new(), Vec::new());
    for (n, placed) in added.iter().enumerate() {
        let (model, its) = spawner.placed(
            commands,
            &mut stores,
            (placed, first + n, !scene.lit),
            (root, &nodes, first),
        );
        roots.push(model);
        nodes.push(its);
    }
    let first_blade = spawner.live_blades(
        commands,
        &mut stores,
        lives,
        (&blades, |model| roots[model]),
    );
    let arms = arms.map(|held| held.map(|held| held.spawned(roots[held.model], first_blade)));
    let body = roots[view.body - first];
    drop(spawner);
    for (name, texture) in textures {
        scene.textures.entry(name).or_insert(texture);
    }
    (view, arms, body, first..scene.models.len())
}

/// The guise `entity` wears in `snapshot`, dressed by the item in its body
/// slot as KOTOR dresses any creature when its armour changes: a script's
/// disguise effect running on it, else its sheet's (a worn item's), else
/// the appearance a script changed it to for good (`ChangeObjectAppearance`;
/// with a head a script gave it, `DuplicateHeadAppearance`, its own
/// appearance too); else, for a creature, its own appearance once what it
/// wears on its body is not what its template put on it (the body the scene
/// drew it in). None while it is drawn in its own body.
fn guise(snapshot: &Snapshot, objects: &Instances, entity: &EntityState) -> Option<Guise> {
    let fighter = entity.fighter.as_ref();
    let look = fighter.map_or(sim::combat::Look::default(), |fighter| fighter.look);
    let facts = objects.of_entity(entity);
    let own = || facts?.body.map(|body| body.appearance);
    let body = snapshot
        .worn(entity.id.0, sim::inventory::slots::BODY)
        .map(str::to_ascii_lowercase);
    let row = fighter
        .and_then(|fighter| fighter.effects.disguise(snapshot.time_ms))
        .or_else(|| crate::cast::sheet(entity, objects).gear.disguise)
        .or(look.appearance)
        .or_else(|| look.head.and_then(|_| own()));
    if let Some(row) = row {
        return Some((row, body, look.head));
    }
    // A player's own body is the wardrobe's to dress (`wardrobe`).
    if !matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)) {
        return None;
    }
    let composed = facts?
        .inventory
        .iter()
        .find(|held| held.slot == Some(sim::inventory::slots::BODY))
        .map(|held| held.template.to_ascii_lowercase());
    (body != composed).then(|| Some((own()?, body, None)))?
}

/// Disguises, each frame the scene has its root: the guise each character
/// wears in the snapshot; a body not read yet for one starts loading, those
/// read are spawned hidden (the cast shows their wearers in them), and the
/// bodies of characters the snapshot no longer has go.
fn track_disguises(
    made: &mut Made,
    (snapshot, objects, level): (&Snapshot, &Instances, &sim::Level),
    (scene, root): (&mut LoadedScene, Entity),
    (lives, assets, commands): (&mut LiveBlades, &mut ModelAssets, &mut Commands),
) {
    made.worn = snapshot
        .all_drawn()
        .filter(|entity| {
            matches!(
                entity.kind,
                Kind::Avatar(_) | Kind::Object(ObjectKind::Creature, _)
            )
        })
        .filter_map(|entity| Some((entity.id, guise(snapshot, objects, entity)?)))
        .collect();
    let gone: Vec<(SimId, Guise)> = made
        .guises
        .keys()
        .filter(|(id, _)| snapshot.drawn(*id).is_none())
        .cloned()
        .collect();
    for key in gone {
        if let Some(Some(guised)) = made.guises.remove(&key) {
            for placed in &mut scene.models[guised.models] {
                placed.ir.meshes = Vec::new();
                placed.clip = None;
            }
            commands.entity(guised.root).despawn();
        }
    }
    let worn = &made.worn;
    made.guises_loading
        .retain(|id, (guise, _)| worn.get(id) == Some(guise));
    for (&id, guise) in &made.worn {
        if !made.guises.contains_key(&(id, guise.clone())) && !made.guises_loading.contains_key(&id)
        {
            let task = made.load_guise(guise.clone(), level);
            made.guises_loading.insert(id, (guise.clone(), task));
        }
    }
    let done: Vec<SimId> = made
        .guises_loading
        .iter()
        .filter(|(_, (_, task))| task.is_finished())
        .map(|(&id, _)| id)
        .collect();
    for id in done {
        let Some((guise, task)) = made.guises_loading.remove(&id) else {
            continue;
        };
        let Some(read) = block_on(poll_once(task)) else {
            continue;
        };
        let guised = match read {
            Ok(mut ready) => {
                made.notes.append(&mut ready.notes);
                let (view, _, body, models) =
                    spawned(ready, (scene, root), (lives, assets, commands));
                commands.entity(body).insert(Visibility::Hidden);
                Some(Guised {
                    root: body,
                    models,
                    view,
                })
            }
            Err(error) => {
                made.notes.push(format!("#{} disguised: {error}", id.0));
                None
            }
        };
        info!(
            "disguise of #{} (appearance {}, wearing {:?}): {}",
            id.0,
            guise.0,
            guise.1,
            if guised.is_some() {
                "drawn"
            } else {
                "not drawn"
            }
        );
        made.guises.insert((id, guise), guised);
    }
}

/// The first of `names`' modules (each one's resolver opened the first time
/// it is asked) that `has` the template, else the first (its refusal is
/// noted).
fn module_with(
    installer: &Installer,
    modules: &Modules,
    names: &[String],
    has: impl Fn(&Module, &Resolver) -> Result<bool, String>,
) -> Result<Arc<Module>, String> {
    let mut chosen = None;
    for name in names {
        let module = modules
            .lock()
            .map_err(|_| "the modules' lock")?
            .entry(name.clone())
            .or_default()
            .clone();
        let resolver = module.resolver.get_or_init(|| installer.resolver(name));
        let found = match resolver {
            Ok(resolver) => has(&module, resolver)?,
            Err(_) => false,
        };
        if found || chosen.is_none() {
            chosen = Some(module);
        }
        if found {
            break;
        }
    }
    chosen.ok_or("no module".into())
}

/// Placeable `template`'s model: `placeables.2da`'s `modelname` by its UTP's
/// `Appearance`, as a module's own placeable is drawn; and the look its
/// `AnimationState` places it in (`sim::look`).
fn prop_model(resolver: &Resolver, template: &str) -> Result<(String, u8), String> {
    let utp = resolver
        .read(template, fmt_odyssey::ResType::UTP)
        .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
        .ok_or(format!("{template}: no UTP"))?;
    let appearance = utp
        .root
        .int("Appearance")
        .and_then(|row| usize::try_from(row).ok())
        .ok_or(format!("{template}: no appearance"))?;
    let placed = utp
        .root
        .int("AnimationState")
        .and_then(|state| u32::try_from(state).ok())
        .map_or(sim::look::DEFAULT, sim::look::placed);
    let model =
        appearance_model(resolver, appearance).map_err(|error| format!("{template}: {error}"))?;
    Ok((model, placed))
}

/// `placeables.2da`'s `modelname` of row `appearance`.
fn appearance_model(resolver: &Resolver, appearance: usize) -> Result<String, String> {
    let table = resolver.two_da("placeables").ok_or("no placeables.2da")?;
    table
        .resref(appearance, "modelname")
        .or_else(|| table.resref(appearance, "model"))
        .ok_or(format!("appearance {appearance} has no model"))
}

/// Made placeables, each frame the scene has its root: those the snapshot
/// has start loading, those read are spawned, those gone leave.
fn track_props(
    made: &mut Made,
    (snapshot, objects, level): (&sim::Snapshot, &Instances, &sim::Level),
    (scene, root): (&mut LoadedScene, Entity),
    (assets, commands, scrolling): (
        &mut ModelAssets,
        &mut Commands,
        &mut crate::scene::ScrollingMaterials,
    ),
) {
    let present: BTreeMap<u32, (&EntityState, &Spawned)> = snapshot
        .all_drawn()
        .filter_map(|entity| {
            let Kind::Object(ObjectKind::Placeable, index) = entity.kind else {
                return None;
            };
            Some((index, (entity, objects.made_from(index)?)))
        })
        .collect();
    let same = |index: &u32, template: &str| {
        present
            .get(index)
            .is_some_and(|(_, spawned)| spawned.template.eq_ignore_ascii_case(template))
    };
    let gone: Vec<u32> = made
        .props
        .iter()
        .filter(|(index, drawn)| !same(index, &drawn.template))
        .map(|(&index, _)| index)
        .collect();
    for index in gone {
        made.depart_prop(index, scene, commands);
    }
    made.props_loading
        .retain(|index, (template, _)| same(index, template));
    for (&index, &(entity, spawned)) in &present {
        if !made.props.contains_key(&index) && !made.props_loading.contains_key(&index) {
            let task = made.load_prop(
                (index, &spawned.template, entity.position, spawned.yaw),
                level,
            );
            made.props_loading
                .insert(index, (spawned.template.clone(), task));
        }
    }
    let done: Vec<u32> = made
        .props_loading
        .iter()
        .filter(|(_, (_, task))| task.is_finished())
        .map(|(&index, _)| index)
        .collect();
    for index in done {
        let Some((template, task)) = made.props_loading.remove(&index) else {
            continue;
        };
        let Some(read) = block_on(poll_once(task)) else {
            continue;
        };
        let models = match read {
            Ok(prop) => Some(Made::spawn_prop(
                prop,
                (scene, root),
                (assets, commands, scrolling),
            )),
            Err(error) => {
                made.notes.push(format!("#{index} {error}"));
                None
            }
        };
        info!(
            "made placeable #{index} ({template}): {}",
            if models.is_some() {
                "drawn"
            } else {
                "not drawn"
            }
        );
        made.props.insert(index, PropDrawn { template, models });
    }
}

/// A placeable, mine or item gone (`drawn`, if the scene had it): its
/// models leave `scene` (their places stay, as other models are named by
/// index) and are despawned.
fn leave(drawn: Option<PropDrawn>, scene: &mut LoadedScene, commands: &mut Commands) {
    let Some(PropDrawn {
        models: Some((root, models)),
        ..
    }) = drawn
    else {
        return;
    };
    for placed in &mut scene.models[models] {
        placed.ir.meshes = Vec::new();
    }
    commands.entity(root).despawn();
}

/// What each module read for the scene's made creatures; one a load
/// holds is "loading" (the frame does not wait for it).
fn read(modules: &Modules) -> BTreeMap<String, serde_json::Value> {
    let Ok(modules) = modules.try_lock() else {
        return BTreeMap::new();
    };
    let facts = |load: &CreatureLoad| {
        serde_json::json!({
            "creature_notes": load.notes,
            "refusals": load.refusals,
            "missing_clips": load.missing_clips,
            "missing_models": load.missing_models,
        })
    };
    modules
        .iter()
        .map(|(name, module)| {
            let read = module
                .load
                .try_lock()
                .map_or(serde_json::json!("loading"), |load| facts(&load));
            (name.clone(), read)
        })
        .collect()
}

/// The party's own mines' dome: the model's hostile red (0.85, 0.18, 0)
/// turned green.
const FRIENDLY: [f32; 3] = [0.18, 0.85, 0.0];
/// The stealth field's visual effect (`nwscript.nss`).
const VFX_DUR_STEALTH_FIELD: u16 = 8002;

/// Detected traps, each frame the scene has its root: those the party now
/// sees start loading their row's model, those read are spawned, those
/// disarmed, sprung or gone leave.
fn track_mines(
    made: &mut Made,
    (snapshot, objects, level): (&sim::Snapshot, &Instances, &sim::Level),
    (scene, root): (&mut LoadedScene, Entity),
    (assets, commands, scrolling): (
        &mut ModelAssets,
        &mut Commands,
        &mut crate::scene::ScrollingMaterials,
    ),
    tints: &mut MineTints,
) {
    use sim::trap as bits;
    let Some(module) = level.module() else {
        return;
    };
    let present: BTreeMap<u32, (String, [f32; 3], bool)> = snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let Kind::Object(ObjectKind::Trigger, index) = entity.kind else {
                return None;
            };
            let state = entity.object?;
            if state.trap & bits::PARTY_SEES == 0 || state.trap & bits::DISABLED != 0 {
                return None;
            }
            let facts = objects.of_entity(entity)?;
            let trap = facts.trap.as_ref()?;
            let row = module.traps.get(usize::from(trap.row))?;
            (!row.model.is_empty()).then_some(())?;
            // A laid mine is the party's; a placed one hostile by its
            // faction.
            let laid = sim::traps::laid_row(&facts.template).is_some();
            let hostile = !laid
                && module
                    .repute
                    .hostile(facts.faction, sim::combat::sheet::factions::PLAYER);
            let at = sim::traps::centre(&facts.geometry, entity.position);
            Some((index, (row.model.clone(), at, !hostile)))
        })
        .collect();
    let same = |index: &u32, model: &str| {
        present
            .get(index)
            .is_some_and(|(wanted, ..)| wanted.eq_ignore_ascii_case(model))
    };
    let gone: Vec<u32> = made
        .mines
        .iter()
        .filter(|(index, drawn)| !same(index, &drawn.template))
        .map(|(&index, _)| index)
        .collect();
    for index in gone {
        leave(made.mines.remove(&index), scene, commands);
    }
    made.mines_loading
        .retain(|index, (model, ..)| same(index, model));
    for (&index, (model, at, friendly)) in &present {
        if !made.mines.contains_key(&index) && !made.mines_loading.contains_key(&index) {
            let task = made.load_mine((model, *at, *friendly), level);
            made.mines_loading.insert(index, (model.clone(), task));
        }
    }
    let done: Vec<u32> = made
        .mines_loading
        .iter()
        .filter(|(_, (.., task))| task.is_finished())
        .map(|(&index, _)| index)
        .collect();
    for index in done {
        let Some((model, task)) = made.mines_loading.remove(&index) else {
            continue;
        };
        let Some(read) = block_on(poll_once(task)) else {
            continue;
        };
        let models = match read {
            Ok(prop) => {
                let spawned = Made::spawn_prop(prop, (scene, root), (assets, commands, scrolling));
                tints.0.push(spawned.0);
                Some(spawned)
            }
            Err(error) => {
                made.notes.push(format!("mine #{index} {error}"));
                None
            }
        };
        info!(
            "detected mine #{index} ({model}): {}",
            if models.is_some() {
                "drawn"
            } else {
                "not drawn"
            }
        );
        made.mines.insert(
            index,
            PropDrawn {
                template: model,
                models,
            },
        );
    }
}

/// The armed traps last seen (seen by the party or not), by id: their
/// row's model and explosion sound, and where they stand (game space,
/// metres), kept past their going so the sound that tells of their
/// springing still finds them; the scene swap they belong to.
#[derive(Default)]
pub struct ArmedTraps {
    generation: u32,
    traps: BTreeMap<u32, (String, String, [f32; 3])>,
}

/// A trap sprung plays its row's model's `activate` where it stood (a gas
/// mine's cloud, a frag mine's burst), seen by the party or not: the sim
/// tells of the springing by the row's `explosionsound` played from the
/// trap (`world::traps::spring`), heard the frame the trap goes.
pub fn sprung(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    heard: Res<crate::ambience::Heard>,
    shots: Option<ResMut<crate::blaster_view::BlasterShots>>,
    mut armed: Local<ArmedTraps>,
) {
    if armed.generation != swap.generation {
        *armed = ArmedTraps {
            generation: swap.generation,
            ..ArmedTraps::default()
        };
    }
    let Some(module) = swap.level.module() else {
        return;
    };
    let snapshot = presented.0.current();
    let objects = Instances::in_snapshot(&swap.level, snapshot);
    for entity in &snapshot.entities {
        let Kind::Object(ObjectKind::Trigger, _) = entity.kind else {
            continue;
        };
        let Some(facts) = objects.of_entity(entity) else {
            continue;
        };
        let row = facts
            .trap
            .as_ref()
            .and_then(|trap| module.traps.get(usize::from(trap.row)));
        if let Some(row) = row.filter(|row| !row.model.is_empty()) {
            let at = sim::traps::centre(&facts.geometry, entity.position);
            armed
                .traps
                .insert(entity.id.0, (row.model.clone(), row.sound.clone(), at));
        }
    }
    let Some(mut shots) = shots else {
        return;
    };
    let now = crate::sim_host::view_ms(&presented.0);
    for sound in &heard.0 {
        let springs = armed
            .traps
            .get(&sound.object)
            .is_some_and(|(_, blast, _)| blast.eq_ignore_ascii_case(&sound.resref()));
        if !springs {
            continue;
        }
        let Some((model, _, at)) = armed.traps.remove(&sound.object) else {
            continue;
        };
        shots.flashes.push(crate::blaster_view::Flash {
            by: SimId(sound.object),
            hand: 0,
            shows: crate::blaster_view::Flashed::Model(model),
            clip: crate::blaster_view::ACTIVATE,
            at: Vec3::from_array(at),
            way: Vec3::Y,
            ms: now,
            scale: 1.0,
        });
    }
}

/// Items lying on the floor (`Goods::floor`), each frame the scene has its
/// root: those the snapshot now has start loading their model, those read
/// are spawned, those taken or destroyed leave.
fn track_floor(
    made: &mut Made,
    (snapshot, level): (&sim::Snapshot, &sim::Level),
    (scene, root): (&mut LoadedScene, Entity),
    (assets, commands, scrolling): (
        &mut ModelAssets,
        &mut Commands,
        &mut crate::scene::ScrollingMaterials,
    ),
) {
    let goods = &snapshot.goods;
    let present: BTreeMap<u32, (&str, sim::inventory::Lying)> = goods
        .floor
        .iter()
        .filter_map(|(id, lying)| {
            let (_, item) = goods.items.iter().find(|(of, _)| of == id)?;
            Some((*id, (item.template.as_str(), *lying)))
        })
        .collect();
    let same = |id: &u32, template: &str| {
        present
            .get(id)
            .is_some_and(|(wanted, _)| wanted.eq_ignore_ascii_case(template))
    };
    let gone: Vec<u32> = made
        .floor
        .iter()
        .filter(|(id, drawn)| !same(id, &drawn.template))
        .map(|(&id, _)| id)
        .collect();
    for id in gone {
        leave(made.floor.remove(&id), scene, commands);
    }
    made.floor_loading
        .retain(|id, (template, _)| same(id, template));
    for (&id, &(template, lying)) in &present {
        if !made.floor.contains_key(&id) && !made.floor_loading.contains_key(&id) {
            let task = made.load_floor_item((template, lying), level);
            made.floor_loading.insert(id, (template.to_string(), task));
        }
    }
    let done: Vec<u32> = made
        .floor_loading
        .iter()
        .filter(|(_, (_, task))| task.is_finished())
        .map(|(&id, _)| id)
        .collect();
    for id in done {
        let Some((template, task)) = made.floor_loading.remove(&id) else {
            continue;
        };
        let Some(read) = block_on(poll_once(task)) else {
            continue;
        };
        let models = match read {
            Ok(prop) => Some(Made::spawn_prop(
                prop,
                (scene, root),
                (assets, commands, scrolling),
            )),
            Err(error) => {
                made.notes.push(format!("item #{id} {error}"));
                None
            }
        };
        info!(
            "item #{id} ({template}) on the floor: {}",
            if models.is_some() {
                "drawn"
            } else {
                "not drawn"
            }
        );
        made.floor.insert(id, PropDrawn { template, models });
    }
}

/// Drawn mines' model roots whose meshes are still to be tinted.
#[derive(Resource, Default)]
pub struct MineTints(pub Vec<Entity>);

/// Tints each newly drawn mine's self-lit meshes (its dome) by their
/// self-illumination (kept until its meshes are there).
pub fn tint_mines(
    mut tints: ResMut<MineTints>,
    children: Query<&Children>,
    meshes: Query<&MeshMaterial3d<crate::material::OdysseyMaterial>>,
    mut materials: ResMut<Assets<crate::material::OdysseyMaterial>>,
) {
    tints.0.retain(|&root| {
        let mut found = false;
        for entity in children.iter_descendants(root) {
            let Ok(handle) = meshes.get(entity) else {
                continue;
            };
            found = true;
            if let Some(mut material) = materials.get_mut(&handle.0) {
                let glow = material.params.self_illum;
                if glow.truncate() != Vec3::ZERO {
                    material.params.tint = glow.truncate().extend(material.params.tint.w);
                }
            }
        }
        !found
    });
}
