//! The players' bodies kept to what the presented snapshot says they are:
//! each seat's persona (`sim::persona`) in the body armour its character
//! wears, as `appearance.2da` dresses a creature (`persona::wear`). A seat
//! that puts on clothes or armour, or is given another persona (a joined
//! player, who loads before it knows the host's), is dressed again: its
//! body and head read on the task pool (`play::dress`), then added to the
//! scene and spawned under its root, and handed to `play_view` (`Redress`),
//! which hides the old body, hangs the seat's weapons in the new one's
//! hands and poses it from then on (`cast::Cast::redress`). What it now
//! wears goes to the run's bodies (`play::Redressed`), so the next
//! module's scene is composed with it.
//!
//! The rule is every character's (`made::guise` dresses any other creature
//! by it): its appearance's body by the item the snapshot says is in its
//! body slot (`Snapshot::worn`, `creature::resolve_appearance`); a seat's
//! body is dressed here because it carries the JA rig and the seat's gait.
//!
//! Render only: the sim decides who wears what, never these models.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use asset_odyssey::model::ModelIr;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use console::{DumpDue, DumpFacts};

use crate::lifecycle::ModuleResolver;
use crate::play::{Dressed, Redressed, Wear};
use crate::scene::{ModelAssets, SceneContent, SceneRoot, Spawner};
use crate::sim_host::{PresentedSnapshot, SceneSwap};

/// What the wardrobe dresses with.
pub struct Setup {
    /// KOTOR 1's resources: the current module's (its own armour and
    /// bodies), else the base's (none: nothing dressed again).
    pub resources: ModuleResolver,
    /// The run's bodies, shared with its scenes' composing.
    pub dressed: Arc<Mutex<Redressed>>,
    /// The weapons every seat was composed holding.
    pub weapons: [Option<ModelIr>; 2],
}

/// A seat dressed again, its body in the scene: for `play_view`.
pub struct Redress {
    pub seat: u8,
    /// The new body's model in the scene (its head after it).
    pub body: usize,
    /// Its fingers' hold (`cast::finger_hold`) and its clips out of a
    /// fight.
    pub hold: Vec<(String, [f32; 4])>,
    pub gait: crate::gait::GaitClips,
    /// What the engine plays on it (`acting::clips`).
    pub acted: Vec<(u16, std::sync::Arc<asset_odyssey::anim::AnimationIr>)>,
    /// The scene swap it was spawned in.
    pub generation: u32,
}

#[derive(Resource)]
pub struct Wardrobe {
    setup: Setup,
    /// Each look's body, read once (`None`: it does not resolve).
    looks: BTreeMap<(u16, Option<String>), Option<Wear>>,
    tables: asset_odyssey::tables::Tables,
    cache: Arc<Mutex<BTreeMap<String, Option<ModelIr>>>>,
    loading: BTreeMap<u8, (Wear, Task<Result<Dressed, String>>)>,
    /// For `play_view`, until it hangs them.
    pub done: Vec<Redress>,
    dressings: usize,
    notes: Vec<String>,
}

impl Wardrobe {
    pub fn new(setup: Setup) -> Wardrobe {
        Wardrobe {
            setup,
            looks: BTreeMap::new(),
            tables: asset_odyssey::tables::Tables::default(),
            cache: Arc::default(),
            loading: BTreeMap::new(),
            done: Vec::new(),
            dressings: 0,
            notes: Vec::new(),
        }
    }

    /// Follows the scene into module `name`: its resources are read from
    /// then on, and nothing read in another module is kept.
    fn enter(&mut self, name: &str) {
        if !self.setup.resources.enter(name) {
            return;
        }
        self.looks.clear();
        self.tables = asset_odyssey::tables::Tables::default();
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// Seat `seat`'s body as `snapshot` has it, if it has a persona.
    fn wanted(&mut self, snapshot: &sim::Snapshot, seat: u8) -> Option<Wear> {
        let worn = sim::persona::dressed(snapshot, sim::Seat(seat))?;
        if let Some(look) = self.looks.get(&worn) {
            return look.clone();
        }
        let resolver = self.setup.resources.get()?;
        let look = crate::persona::wear(&resolver, &mut self.tables, worn.clone());
        self.looks.insert(worn, look.clone());
        look
    }

    fn load(&self, wear: Wear) -> Option<Task<Result<Dressed, String>>> {
        let resources = self.setup.resources.clone();
        let cache = self.cache.clone();
        Some(AsyncComputeTaskPool::get().spawn(async move {
            let resolver = resources.get().ok_or("no resources")?;
            let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
            crate::play::dress(&resolver, &mut cache, &wear, [0.0; 3])
        }))
    }
}

/// Each frame, before `play_view` draws: seats whose body is not the one
/// the snapshot gives them start loading it; those loaded are spawned (see
/// the module documentation).
#[allow(clippy::too_many_arguments)]
pub fn dress(
    mut wardrobe: ResMut<Wardrobe>,
    presented: Res<PresentedSnapshot>,
    (swap, mut content): (Res<SceneSwap>, ResMut<SceneContent>),
    roots: Query<Entity, With<SceneRoot>>,
    mut assets: ModelAssets,
    mut commands: Commands,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
) {
    let wardrobe = &mut *wardrobe;
    wardrobe.enter(swap.level.name());
    let snapshot = presented.0.current();
    let dressed = wardrobe.setup.dressed.clone();
    let worn: Vec<Wear> = dressed
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .wears
        .clone();
    for (seat, now) in worn.iter().enumerate() {
        let seat = seat as u8;
        let Some(wanted) = wardrobe.wanted(snapshot, seat) else {
            continue;
        };
        if wanted == *now {
            wardrobe.loading.remove(&seat);
            continue;
        }
        if wardrobe
            .loading
            .get(&seat)
            .is_some_and(|(wear, _)| *wear == wanted)
        {
            continue;
        }
        if let Some(task) = wardrobe.load(wanted.clone()) {
            wardrobe.loading.insert(seat, (wanted, task));
        }
    }
    if let Some(root) = roots.iter().next() {
        let finished: Vec<u8> = wardrobe
            .loading
            .iter()
            .filter(|(_, (_, task))| task.is_finished())
            .map(|(&seat, _)| seat)
            .collect();
        for seat in finished {
            let Some((wear, task)) = wardrobe.loading.remove(&seat) else {
                continue;
            };
            match block_on(poll_once(task)) {
                Some(Ok(read)) => {
                    let scene = &mut *content.bypass_change_detection().0;
                    let body = spawn(
                        (seat, &wear, read),
                        (&wardrobe.setup, scene, root),
                        (&mut assets, &mut commands),
                    );
                    wardrobe.dressings += 1;
                    wardrobe.done.push(Redress {
                        generation: swap.generation,
                        ..body
                    });
                }
                Some(Err(error)) => wardrobe.notes.push(format!("{}: {error}", wear.body)),
                None => {}
            }
        }
    }
    if dump_due.0 {
        facts.0.insert(
            "wardrobe".into(),
            serde_json::json!({
                "wears": worn.iter().map(|wear| format!("{} {} {:?}", wear.body, wear.head, wear.texture)).collect::<Vec<_>>(),
                "loading": wardrobe.loading.keys().collect::<Vec<_>>(),
                "dressings": wardrobe.dressings,
                "notes": wardrobe.notes,
            }),
        );
    }
}

/// Seat `seat`'s new body (`read`) added to `scene` and spawned under
/// `root`, and kept for the scenes composed from now on.
fn spawn(
    (seat, wear, read): (u8, &Wear, Dressed),
    (setup, scene, root): (&Setup, &mut crate::viewer::LoadedScene, Entity),
    (assets, commands): (&mut ModelAssets, &mut Commands),
) -> Redress {
    let Dressed {
        mut models,
        textures,
        hold,
        gait,
        acted,
        ..
    } = read;
    let first = scene.models.len();
    scene
        .models
        .extend(models.iter().cloned().map(|mut placed| {
            if let Some((body, _)) = &mut placed.hook {
                *body += first;
            }
            placed
        }));
    let (images, mut stores) = assets.split();
    let mut spawner = Spawner::new(&textures, scene.area_light(), images);
    let mut nodes = Vec::new();
    for n in first..scene.models.len() {
        let (_, its) = spawner.placed(
            commands,
            &mut stores,
            (&scene.models[n], n, !scene.lit),
            (root, &nodes, first),
        );
        nodes.push(its);
    }
    drop(spawner);
    for (name, texture) in &textures {
        scene
            .textures
            .entry(name.clone())
            .or_insert_with(|| texture.clone());
    }
    // As a scene composes a seat: the weapons in the new body's hands.
    crate::play::hang(&mut models, &setup.weapons, None);
    let mut dressed = setup.dressed.lock().unwrap_or_else(PoisonError::into_inner);
    dressed.models.insert(seat, models);
    dressed.textures.extend(textures);
    if let Some(now) = dressed.wears.get_mut(usize::from(seat)) {
        *now = wear.clone();
    }
    Redress {
        seat,
        body: first,
        hold,
        gait,
        acted,
        generation: 0,
    }
}
