//! The characters' weapons in `kjk play`, kept to their sheets in the
//! presented snapshots. A scene is composed with every seat holding the
//! local sheet's weapons and every creature its template's (`play::compose`);
//! a character whose snapshot sheet names other weapons (a joined player,
//! who loads before it knows the host's sheets; later, a weapon equipped, or
//! a lightsaber's colour crystal changed; a creature's script drawing the
//! blade in its pack, as K1's AI has a gunner do with a foe within 3 m) is
//! armed again: its weapon models (the worn item's, read through the
//! current module's resources as its templates are, else each base item
//! at model variation 1, `weapons::sheet_models`) read and readied on a
//! thread (`weapons::arm`: a lightsaber's planes give
//! way to JA blades), then spawned under the scene's root
//! (`scene::Spawner`) and handed to `play_view` (`Rearms`), which hangs
//! them in the hands (`cast::Cast::rearm`) and retires the old ones. What
//! a character holds is noted once its load is done; one that failed is
//! not tried again in that module.
//!
//! Render only: the sim reads the sheets, never these models.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use asset_odyssey::model::ModelIr;
use asset_odyssey::texture::TextureIr;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use console::DumpFacts;
use sim::combat::sheet::Sheet;
use sim::{Instances, Kind, SimId};

use crate::lifecycle::ModuleResolver;
use crate::scene::{LiveBlades, ModelAssets, SceneContent, SceneRoot, Spawner};
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::viewer::{self, BladeSpec, Placed};
use crate::weapons::{self, Armed, Arms, Owner, Sabers};

/// Which weapons a character holds: each slot's base item, and the
/// template of the item worn in that hand.
type Holding = [Option<(u16, Option<String>)>; 2];

fn holding(sheet: &Sheet, worn: &[Option<String>; 2]) -> Holding {
    std::array::from_fn(|slot| {
        sheet.weapons[slot].map(|weapon| (weapon.base_item, worn[slot].clone()))
    })
}

/// The templates worn in the right and left hands, of `worn` (each item's
/// slot and template).
fn in_hands<'a>(
    worn: impl Iterator<Item = (Option<u8>, &'a String)> + Clone,
) -> [Option<String>; 2] {
    use sim::inventory::slots::{LEFT_WEAPON, RIGHT_WEAPON};
    [RIGHT_WEAPON, LEFT_WEAPON].map(|slot| {
        worn.clone()
            .find(|(at, _)| *at == Some(slot))
            .map(|(_, template)| template.clone())
    })
}

/// The templates `body` wears in its right and left hands.
fn worn_in_hands(goods: &sim::inventory::Goods, body: u32) -> [Option<String>; 2] {
    in_hands(
        goods
            .items
            .iter()
            .filter(|(_, item)| item.holder == body)
            .map(|(_, item)| (item.slot, &item.template)),
    )
}

/// The templates a creature not of the party wears in its right and left
/// hands, as `sheet` has them (the snapshot's goods are the party's): of
/// the items it started with, the first of each hand's base item.
fn creature_worn(
    sheet: &Sheet,
    facts: &sim::module::ObjectData,
    catalog: &sim::inventory::Catalog,
) -> [Option<String>; 2] {
    std::array::from_fn(|slot| {
        let base = sheet.weapons[slot]?.base_item;
        facts
            .inventory
            .iter()
            .find(|held| {
                catalog
                    .get(&held.template)
                    .is_some_and(|item| item.base_item == base)
            })
            .map(|held| held.template.clone())
    })
}

/// The templates `entity` (its sheet `sheet`) wears in its right and left
/// hands in `snapshot`: the party's goods say, else, for a creature not of
/// the party, its own items (`creature_worn`).
pub(crate) fn worn(
    snapshot: &sim::Snapshot,
    objects: &Instances,
    catalog: Option<&sim::inventory::Catalog>,
    (entity, sheet): (&sim::EntityState, &Sheet),
) -> [Option<String>; 2] {
    let worn = worn_in_hands(&snapshot.goods, entity.id.0);
    if let (Kind::Object(_, index), [None, None]) = (entity.kind, &worn)
        && let (Some(facts), Some(catalog)) = (objects.get(index), catalog)
    {
        return creature_worn(sheet, facts, catalog);
    }
    worn
}

/// What a creature was composed holding: its instance's sheet's weapons and
/// the items its template equips.
fn composed_creature(facts: &sim::module::ObjectData) -> Holding {
    let worn = in_hands(
        facts
            .inventory
            .iter()
            .map(|held| (held.slot, &held.template)),
    );
    holding(&facts.sheet.unwrap_or_else(Sheet::commoner), &worn)
}

/// What the armory needs from the run's load.
pub struct Setup {
    /// KOTOR 1's resources, where weapon models come from: the current
    /// module's (its own templates and models: a pack's vibroblade only one
    /// module has), else the base's (none: no K1, nothing armed again).
    pub resources: ModuleResolver,
    pub sabers: Sabers,
    /// The JA blade shaders' textures, for blades armed later.
    pub blade_textures: BTreeMap<String, TextureIr>,
    /// The sheet every seat's weapons were composed from.
    pub composed: Sheet,
}

/// A character's weapons, spawned, to hang in its hands: a seat's in its
/// body's then, a creature's in its own (`id`).
pub struct Rearm {
    pub owner: Owner,
    pub id: SimId,
    pub sheet: Sheet,
    pub arms: [Option<Armed>; 2],
    /// The scene swap they were spawned in.
    pub generation: u32,
    /// Whether its body plays its wield's draw (`acting::Once`): a
    /// creature's weapons changed as it stood in the scene, not as it was
    /// first seen there.
    pub draws: bool,
}

impl Rearm {
    /// Who takes them: the character whose sheet names them, even while its
    /// seat plays a companion (a save made so, or an item equipped so).
    pub fn holder(&self) -> (Owner, Option<SimId>) {
        (self.owner, Some(self.id))
    }
}

/// Characters armed this frame, for `play_view`.
#[derive(Resource, Default)]
pub struct Rearms(pub Vec<Rearm>);

/// A character's weapon models read and readied off the main thread: the
/// models, the arms (by index among them), their blades, and textures.
struct Ready {
    models: Vec<Placed>,
    arms: Arms,
    blades: Vec<BladeSpec>,
    textures: BTreeMap<String, TextureIr>,
    missing: Vec<String>,
    ms: f64,
}

/// A character being armed: for what, by which sheet, on which thread.
struct Underway {
    id: SimId,
    holding: Holding,
    sheet: Sheet,
    draws: bool,
    task: Task<Result<Ready, String>>,
}

#[derive(Resource)]
pub struct Armory {
    setup: Setup,
    /// Models read so far, by name (shared with the threads).
    cache: Arc<Mutex<BTreeMap<String, Option<ModelIr>>>>,
    generation: u32,
    /// What each character holds when it differs from what it was
    /// composed with.
    holds: BTreeMap<Owner, Holding>,
    /// What a load failed to give each character, in this module.
    failed: BTreeMap<Owner, Holding>,
    /// What each character was last seen wanting to hold, in this scene.
    seen: BTreeMap<Owner, Holding>,
    /// Characters being armed.
    loading: BTreeMap<Owner, Underway>,
    armed: usize,
    /// Characters armed again with a draw, and at which tick.
    drawn: Vec<(Owner, u64)>,
    last_ms: f64,
    notes: Vec<String>,
}

impl Armory {
    pub fn new(setup: Setup) -> Armory {
        Armory {
            setup,
            cache: Arc::default(),
            generation: 0,
            holds: BTreeMap::new(),
            failed: BTreeMap::new(),
            seen: BTreeMap::new(),
            loading: BTreeMap::new(),
            armed: 0,
            drawn: Vec::new(),
            last_ms: 0.0,
            notes: Vec::new(),
        }
    }

    fn held(&self, owner: Owner, composed: impl FnOnce() -> Holding) -> Holding {
        self.holds.get(&owner).cloned().unwrap_or_else(composed)
    }

    /// Follows the scene into module `name`: its resources are read from
    /// then on (a model one module has may differ from another's of the
    /// name, so none read before is kept), and what failed may load there.
    fn enter(&mut self, name: &str) {
        if !self.setup.resources.enter(name) {
            return;
        }
        self.failed.clear();
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Reads and readies `sheet`'s weapon models (those `worn`) on a
    /// thread, through the current module's resources where it has them.
    fn load(&self, sheet: Sheet, worn: [Option<String>; 2]) -> Option<Task<Result<Ready, String>>> {
        let resources = self.setup.resources.clone();
        if !resources.is_some() {
            return None;
        }
        let cache = self.cache.clone();
        let sabers = self.setup.sabers.clone();
        let blade_textures = self.setup.blade_textures.clone();
        Some(AsyncComputeTaskPool::get().spawn(async move {
            let started = Instant::now();
            let resolver = resources.get().ok_or("no resources")?;
            let found = {
                let mut cache = cache.lock().map_err(|_| "the model cache lock")?;
                weapons::sheet_models(&resolver, &mut cache, &sheet, &worn)?
            };
            if found.iter().all(Option::is_none) && sheet.weapons.iter().any(Option::is_some) {
                return Err(format!("no weapon model ({worn:?})"));
            }
            let mut models: Vec<Placed> = Vec::new();
            let slots = found.map(|ir| {
                let ir = ir?;
                models.push(Placed::at(ir, [0.0; 3]));
                Some(models.len() - 1)
            });
            let mut blades = Vec::new();
            let arms = weapons::arm(&mut models, &mut blades, slots, &sabers, &mut Vec::new());
            let (mut textures, missing) = viewer::textures(&resolver, &models);
            textures.extend(blade_textures);
            Ok(Ready {
                models,
                arms,
                blades,
                textures,
                missing,
                ms: started.elapsed().as_secs_f64() * 1000.0,
            })
        }))
    }
}

/// Each frame: characters whose sheets name other weapons than they hold
/// start loading them; loads done are spawned and handed on (`Rearms`). A
/// creature is looked at only once its sheet is in the snapshot (its
/// equipment changed).
#[allow(clippy::too_many_arguments)]
pub fn arm(
    mut armory: ResMut<Armory>,
    presented: Res<PresentedSnapshot>,
    (swap, scene): (Res<SceneSwap>, Res<SceneContent>),
    roots: Query<Entity, With<SceneRoot>>,
    (mut rearms, mut lives): (ResMut<Rearms>, ResMut<LiveBlades>),
    mut commands: Commands,
    mut assets: ModelAssets,
    mut facts: ResMut<DumpFacts>,
) {
    let armory = &mut *armory;
    // A new scene holds the composed weapons again; loads under way are
    // dropped with it. In the same scene the seats hold what they held.
    if swap.generation != armory.generation {
        armory.generation = swap.generation;
        if swap.scene {
            armory.holds.clear();
            armory.loading.clear();
            armory.seen.clear();
        }
    }
    armory.enter(swap.level.resources());
    let current = presented.0.current();
    let objects = Instances::in_snapshot(&swap.level, current);
    let catalog = swap.level.scripts().map(|library| library.items());
    for entity in &current.entities {
        let Some(sheet) = entity.sheet else {
            continue;
        };
        let owner = match entity.kind {
            Kind::Avatar(seat) => Owner::Seat(seat.0),
            Kind::Object(_, index) => Owner::Creature(index),
            _ => continue,
        };
        let worn = worn(current, &objects, catalog, (entity, &sheet));
        let wanted = holding(&sheet, &worn);
        // The exe plays the new wield's draw as a slot's weapon changes
        // (its equip handler; players' characters draw as JA's do).
        let changed = matches!(owner, Owner::Creature(_))
            && armory
                .seen
                .insert(owner, wanted.clone())
                .is_some_and(|was| was != wanted);
        let held = armory.held(owner, || match owner {
            Owner::Seat(_) => holding(&armory.setup.composed, &[None, None]),
            Owner::Creature(index) => objects.get(index).map_or([None, None], composed_creature),
        });
        let underway = armory
            .loading
            .get(&owner)
            .map(|underway| underway.holding.clone());
        if wanted == underway.unwrap_or(held.clone()) {
            continue;
        }
        if wanted == held {
            // Back to what it holds before the other load came in.
            armory.loading.remove(&owner);
            continue;
        }
        if armory.failed.get(&owner) == Some(&wanted) {
            continue;
        }
        if let Some(task) = armory.load(sheet, worn) {
            let draws = changed
                || armory
                    .loading
                    .get(&owner)
                    .is_some_and(|underway| underway.draws);
            armory.loading.insert(
                owner,
                Underway {
                    id: entity.id,
                    holding: wanted,
                    sheet,
                    draws,
                    task,
                },
            );
        }
    }
    let Some(root) = roots.iter().next() else {
        return;
    };
    let done: Vec<Owner> = armory
        .loading
        .iter_mut()
        .filter_map(|(owner, underway)| underway.task.is_finished().then_some(*owner))
        .collect();
    for owner in done {
        let Some(Underway {
            id,
            holding,
            sheet,
            draws,
            task,
        }) = armory.loading.remove(&owner)
        else {
            continue;
        };
        let ready = match block_on(poll_once(task)) {
            Some(Ok(ready)) => ready,
            Some(Err(error)) => {
                armory.notes.push(format!("{owner:?}: {error}"));
                armory.failed.insert(owner, holding);
                continue;
            }
            None => continue,
        };
        armory.holds.insert(owner, holding);
        let (images, mut stores) = assets.split();
        let mut spawner = Spawner::new(&ready.textures, scene.0.area_light(), images);
        // Out of the scene's model indices: never a body's or a room's.
        let spawned: Vec<Entity> = ready
            .models
            .iter()
            .map(|placed| {
                let at = (root, Transform::IDENTITY);
                let (model, _) =
                    spawner.model(&mut commands, &mut stores, (placed, usize::MAX, false), at);
                // Shown once hung in a hand (`weapons::Wielded::draw`).
                commands.entity(model).insert(Visibility::Hidden);
                model
            })
            .collect();
        let first = spawner.live_blades(
            &mut commands,
            &mut stores,
            &mut lives,
            (&ready.blades, |model| spawned[model]),
        );
        let arms = ready
            .arms
            .map(|held| held.map(|held| held.spawned(spawned[held.model], first)));
        armory.armed += 1;
        if draws {
            armory.drawn.push((owner, current.tick));
        }
        armory.last_ms = ready.ms;
        armory.notes.extend(
            ready
                .missing
                .iter()
                .map(|name| format!("{owner:?}: no texture {name}")),
        );
        rearms.0.push(Rearm {
            owner,
            id,
            sheet,
            arms,
            generation: armory.generation,
            draws,
        });
    }
    facts.0.insert(
        "armory".into(),
        serde_json::json!({
            "armed": armory.armed,
            "drawn": armory.drawn.iter().map(|(owner, ms)| format!("{owner:?} at tick {ms}")).collect::<Vec<_>>(),
            "loading": armory.loading.len(),
            "last_load_ms": armory.last_ms,
            "holds": armory
                .holds
                .iter()
                .map(|(owner, holding)| (format!("{owner:?}"), holding))
                .collect::<BTreeMap<_, _>>(),
            "notes": armory.notes,
        }),
    );
}
