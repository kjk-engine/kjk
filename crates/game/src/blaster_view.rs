//! KOTOR's blaster fire as its executable draws it
//! (`context/artifacts/kotor-blasters/SURVEY.md`), over what the other
//! views ask for each frame (`BlasterShots`):
//!
//! - A bolt: its `ammunitiontypes.2da` row's model (`model0`, a Power
//!   Blast's `model1`) where the bolt is drawn, its +Y along its flight,
//!   playing `travel01` from when it is first shown: the clip's `detonate`
//!   bursts its `zap` (a streak riding with it, `model_fx`), and its light
//!   glows as the clip keys it (lit as it leaves the gun, out by a third of
//!   a second) among the scene's lights (`ShotLights`). A bolt no longer
//!   asked for vanishes with its particles, as the executable deletes its
//!   projectile on arrival (no bolt model has an `impact01`).
//! - A muzzle flash: the row's `muzzleflash` model on the shooter's gun's
//!   muzzle as drawn, following it, along the shot, playing its `impact`
//!   once; smaller before the eyes the camera looks through.
//! - A feat's flash: the `Imp_Impact_Node` model of the feat's row of
//!   `visualeffects.2da` (`FEAT_ROWS`) on the gun the same way.
//! - A tracer (the disruptor, a hitscan): its bolt model drawn flying from
//!   the gun to where the shot ended at `TRACER_SPEED`.
//!
//! Each model is read once a scene, and drawn from a pool of copies made
//! hidden as it is read (`SPARE` of them, more as more show at once), a
//! copy made new for each bolt or flash.

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_odyssey::texture::TextureIr;
use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use fmt_odyssey::mdl;
use sim::SimId;
use sim::combat::specials::Special;
use sim::minigame::Anim;

use crate::cast::Guns;
use crate::fx_model::{self, Drawing, Model, Spawned, Spawning};
use crate::heat_haze::HeatHazeMaterial;
use crate::lifecycle::Installer;
use crate::lifecycle_view::SceneCamera;
use crate::material::GpuLight;
use crate::scene::{ModelAssets, SceneContent, SceneRoot, Spawner, game_to_bevy};
use crate::script_fx::Loaded;
use crate::sim_host::{PresentedSnapshot, SceneSwap, view_ms};

/// How fast a tracer is drawn flying (metres a second): a disruptor's
/// bolt, its shot a hitscan, about twice a rifle's bolt.
const TRACER_SPEED: f32 = 120.0;
/// Copies of each model kept at most: the most of one shown at once.
const COPIES: usize = 48;
/// Copies of each model kept idle, made as the model is read and again
/// whenever fewer are free (a frame later): every copy is entities that
/// each frame's visibility, transforms and extraction pass over, so a
/// pool grows only as far as the fire needs.
const SPARE: usize = 4;
/// A bolt's clip in flight, a flash's, a sprung mine's (`made::sprung`).
const TRAVEL: &str = "travel01";
pub(crate) const IMPACT: &str = "impact";
pub(crate) const ACTIVATE: &str = "activate";
/// How long a flash without its clip is kept (ms).
const FLASH_MS: f64 = 2000.0;
/// The colour controller of a light (reone's controller numbers).
const LIGHT_COLOUR: u32 = 76;
/// The feats' visuals on the gun as their shots leave it (the executable's
/// table, fn 0x10029f69e): Power Blast's (Improved, Master), Sniper
/// Shot's (its first too), Rapid Shot's (Improved, Multi Shot).
const FEAT_ROWS: [u16; 7] = [4013, 4029, 4038, 4015, 4020, 4022, 4016];

/// The feat's visual a special's shot plays on the gun (`FEAT_ROWS`): none
/// for Power Blast's and Rapid Shot's first.
pub fn feat_row(special: Special) -> Option<u16> {
    match special {
        Special::PowerBlast(2) => Some(4013),
        Special::PowerBlast(3) => Some(4029),
        Special::SniperShot(1) => Some(4038),
        Special::SniperShot(2) => Some(4015),
        Special::SniperShot(3) => Some(4020),
        Special::RapidShot(2) => Some(4022),
        Special::RapidShot(3) => Some(4016),
        _ => None,
    }
}

/// A bolt shown this frame: which (its key stays while it flies), its
/// model, where (game space, metres) and which way it flies.
pub struct BoltShown {
    pub key: (u32, u64),
    pub model: String,
    pub at: Vec3,
    pub way: Vec3,
}

/// What a flash shows: a model by name, or a feat's row.
pub enum Flashed {
    Model(String),
    Row(u16),
}

/// A flash on `by`'s gun in `hand` (`cast::Guns`) from `ms` (the view's),
/// playing `clip` once: where the gun is not drawn (a sprung mine has
/// none), at `at` (game space, metres); along `way`, `scale` times its
/// size.
pub struct Flash {
    pub by: SimId,
    pub hand: u8,
    pub shows: Flashed,
    pub clip: &'static str,
    pub at: Vec3,
    pub way: Vec3,
    pub ms: f64,
    pub scale: f32,
}

/// A tracer from `from` to `to` (game space, metres) from `ms` (the view's).
pub struct Tracer {
    pub model: String,
    pub from: Vec3,
    pub to: Vec3,
    pub ms: f64,
}

/// What the views asked for since the last frame (`combat_fx`, `shot_fx`).
#[derive(Resource, Default)]
pub struct BlasterShots {
    pub bolts: Vec<BoltShown>,
    pub flashes: Vec<Flash>,
    pub tracers: Vec<Tracer>,
}

/// The bolts' lights shown, in Bevy's space, as of this frame: candidates
/// beside the scene's (`scene::pick_lights`).
#[derive(Resource, Default, PartialEq)]
pub(crate) struct ShotLights(pub Vec<GpuLight>);

/// A model read: the model, its textures, and its light's colour as its
/// `travel01` keys it (seconds, colour).
struct Read {
    model: Model,
    textures: BTreeMap<String, TextureIr>,
    light: Vec<(f32, Vec3)>,
}

/// A copy of a model, and what it shows: its key, and when it began (the
/// view's ms).
struct Pooled {
    spawned: Spawned,
    shows: Option<(u64, f64)>,
}

/// A flash or tracer in play.
struct Live {
    model: String,
    key: u64,
    kind: LiveKind,
    since: f64,
}

enum LiveKind {
    Flash {
        by: SimId,
        hand: u8,
        clip: &'static str,
        at: Vec3,
        way: Vec3,
        scale: f32,
    },
    Tracer {
        from: Vec3,
        to: Vec3,
    },
}

#[derive(Resource)]
pub struct BlasterView {
    installer: Option<Arc<Installer>>,
    generation: u32,
    /// The feats' rows' models, by row.
    rows: Option<Loaded<BTreeMap<u16, String>>>,
    models: BTreeMap<String, Loaded<Option<Arc<Read>>>>,
    pools: BTreeMap<String, Vec<Pooled>>,
    live: Vec<Live>,
    /// Keys handed to flashes and tracers.
    next: u64,
    drawn_ms: Option<f64>,
    /// Frames bolts, flashes and tracers were shown in, each counted (the
    /// dumps).
    shown: [usize; 3],
}

/// A session's blaster fire, its models read from `installer`.
pub fn install(world: &mut World, installer: Option<Arc<Installer>>) {
    world.insert_resource(BlasterView {
        installer,
        generation: 0,
        rows: None,
        models: BTreeMap::new(),
        pools: BTreeMap::new(),
        live: Vec::new(),
        next: 0,
        drawn_ms: None,
        shown: [0; 3],
    });
}

pub struct BlasterViewPlugin;

impl Plugin for BlasterViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlasterShots>()
            .init_resource::<ShotLights>()
            .add_systems(
                PostUpdate,
                (spawn, draw)
                    .chain()
                    .run_if(resource_exists::<BlasterView>)
                    .run_if(resource_exists::<PresentedSnapshot>)
                    .after(crate::combat_fx::FightDrawn)
                    .before(bevy::asset::AssetEventSystems),
            )
            .add_systems(
                PostUpdate,
                forget
                    .run_if(not(resource_exists::<BlasterView>))
                    .after(crate::combat_fx::FightDrawn),
            );
    }
}

/// With nothing to draw them, the asks are dropped.
fn forget(mut shots: ResMut<BlasterShots>) {
    *shots = BlasterShots::default();
}

/// Model `name` read for `module`, with its light's keys.
fn read(installer: &Installer, module: &str, name: &str) -> Option<Arc<Read>> {
    let resolver = installer.resolver(module).ok()?;
    let model = fx_model::load(&resolver, name, false)?;
    let textures = fx_model::textures(&resolver, std::iter::once(&model));
    let light = crate::viewer::parsed(&resolver, name)
        .ok()
        .flatten()
        .map_or_else(Vec::new, |parsed| light_keys(&parsed));
    Some(Arc::new(Read {
        model,
        textures,
        light,
    }))
}

/// The colour `travel01` keys on the model's first light.
fn light_keys(model: &mdl::Model) -> Vec<(f32, Vec3)> {
    fn find(node: &mdl::Node) -> Option<&mdl::Controller> {
        node.controllers
            .iter()
            .find(|c| c.kind == LIGHT_COLOUR && c.columns == 3 && !c.bezier)
            .or_else(|| node.children.iter().find_map(find))
    }
    let Some(clip) = model
        .animations
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(TRAVEL))
    else {
        return Vec::new();
    };
    find(&clip.root).map_or_else(Vec::new, |keys| {
        keys.times
            .iter()
            .zip(keys.values.as_chunks::<3>().0)
            .map(|(&t, rgb)| (t, Vec3::from_array(*rgb)))
            .collect()
    })
}

/// The colour `keys` give `at` seconds in (held past the last).
fn colour_at(keys: &[(f32, Vec3)], at: f32) -> Option<Vec3> {
    let after = keys.iter().position(|&(t, _)| t > at);
    match after {
        Some(0) => keys.first().map(|k| k.1),
        Some(k) => {
            let ((t0, c0), (t1, c1)) = (keys[k - 1], keys[k]);
            Some(c0.lerp(c1, (at - t0) / (t1 - t0).max(f32::EPSILON)))
        }
        None => keys.last().map(|k| k.1),
    }
}

/// The feats' rows' `Imp_Impact_Node` models.
fn feat_models(installer: &Installer, module: &str) -> BTreeMap<u16, String> {
    let Some(table) = installer
        .resolver(module)
        .ok()
        .and_then(|resolver| resolver.two_da("visualeffects"))
    else {
        return BTreeMap::new();
    };
    (0..table.cells.len())
        .filter_map(|row| {
            let label = String::from_utf8_lossy(table.row_labels.get(row)?);
            let id = label.trim().parse::<u16>().ok()?;
            FEAT_ROWS.contains(&id).then_some(())?;
            Some((id, table.resref(row, "imp_impact_node")?))
        })
        .collect()
}

/// Reads the models asked for and makes each one's copies.
#[allow(clippy::too_many_arguments)]
fn spawn(
    mut view: ResMut<BlasterView>,
    swap: Res<SceneSwap>,
    scene: Res<SceneContent>,
    (shots, kotor): (
        Res<BlasterShots>,
        Option<Res<crate::weapon_sounds::WeaponSounds>>,
    ),
    roots: Query<Entity, With<SceneRoot>>,
    mut assets: ModelAssets,
    mut hazes: ResMut<Assets<HeatHazeMaterial>>,
    mut commands: Commands,
) {
    let view = &mut *view;
    if swap.generation != view.generation {
        // Their copies went with the old scene.
        view.generation = swap.generation;
        view.rows = None;
        view.models.clear();
        view.pools.clear();
        view.live.clear();
        view.drawn_ms = None;
    }
    let Some(installer) = view.installer.clone() else {
        return;
    };
    let module = swap.level.resources().to_string();
    let rows = view.rows.get_or_insert_with(|| {
        let (installer, module) = (installer.clone(), module.clone());
        Loaded::Loading(
            AsyncComputeTaskPool::get().spawn(async move { feat_models(&installer, &module) }),
        )
    });
    let rows = rows.poll().cloned().unwrap_or_else(BTreeMap::new);
    let named = |shows: &Flashed| match shows {
        Flashed::Model(name) => Some(name.clone()),
        Flashed::Row(row) => rows.get(row).cloned(),
    };
    // Every blaster's, feat's and mine's model read as the scene comes, so
    // the first shot or blast shows (and what is asked for that is not
    // among them).
    let mines = swap
        .level
        .module()
        .into_iter()
        .flat_map(|module| &module.traps);
    let wanted: Vec<String> = (kotor.iter().flat_map(|kotor| kotor.blaster_models()))
        .chain(rows.values().cloned())
        .chain(
            mines
                .filter(|row| !row.model.is_empty())
                .map(|row| row.model.clone()),
        )
        .chain(shots.bolts.iter().map(|bolt| bolt.model.clone()))
        .chain(shots.flashes.iter().filter_map(|flash| named(&flash.shows)))
        .chain(shots.tracers.iter().map(|tracer| tracer.model.clone()))
        .collect();
    crate::script_fx::ask_for(&mut view.models, wanted, (&installer, &module), read);
    let Some(root) = roots.iter().next() else {
        return;
    };
    for (name, model) in &mut view.models {
        let Some(Some(read)) = model.poll() else {
            continue;
        };
        let copies = view.pools.entry(name.clone()).or_default();
        let idle = copies.iter().filter(|copy| copy.shows.is_none()).count();
        let more = SPARE.saturating_sub(idle).min(COPIES - copies.len());
        if more == 0 {
            continue;
        }
        let (images, stores) = assets.split();
        let mut spawning = Spawning {
            spawner: Spawner::new(&read.textures, scene.0.area_light(), images),
            stores,
            hazes: &mut hazes,
            textures: &read.textures,
            area: scene.0.area_light(),
            lit: scene.0.lit,
            root,
            // Each batch's emitters seeded on from the copies before it.
            seed: 0x5eed ^ (copies.len() as u32).wrapping_mul(0x9e37_79b9),
            commands: &mut commands,
        };
        copies.extend((0..more).map(|_| Pooled {
            spawned: spawning.spawn(read.model.copy(), true),
            shows: None,
        }));
    }
}

/// One thing to show this frame in a copy of `model`: its key, where,
/// which way, how big, its clip, and when it began (the view's ms).
struct Shown {
    model: String,
    key: u64,
    at: Vec3,
    way: Vec3,
    scale: f32,
    clip: &'static str,
    since: f64,
}

/// The copy of `copies` to show `key` in: the one that showed it last,
/// else a free one, else the one showing the oldest.
fn pick(copies: &[Pooled], taken: &[bool], key: u64) -> Option<usize> {
    let open = |k: &usize| !taken[*k];
    let mine = (0..copies.len())
        .filter(open)
        .find(|&k| copies[k].shows.is_some_and(|(shown, _)| shown == key));
    mine.or_else(|| {
        (0..copies.len())
            .filter(open)
            .find(|&k| copies[k].shows.is_none())
    })
    .or_else(|| {
        (0..copies.len()).filter(open).min_by(|&a, &b| {
            let since = |k: usize| copies[k].shows.map_or(f64::MIN, |(_, since)| since);
            since(a).total_cmp(&since(b))
        })
    })
}

/// Shows what was asked for in the models' copies and draws them; their
/// lights join the scene's.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut view: ResMut<BlasterView>,
    mut shots: ResMut<BlasterShots>,
    presented: Res<PresentedSnapshot>,
    (guns, places): (Res<Guns>, Query<&GlobalTransform>),
    cameras: Query<Entity, SceneCamera>,
    mut posing: ParamSet<(Query<&mut Transform>, Drawing)>,
    (mut lights, mut facts): (ResMut<ShotLights>, ResMut<console::DumpFacts>),
) {
    let view = &mut *view;
    let shots = std::mem::take(&mut *shots);
    let now = view_ms(&presented.0);
    let dt = view
        .drawn_ms
        .replace(now)
        .filter(|was| *was <= now)
        .map_or(0.0, |was| ((now - was) / 1000.0) as f32);
    let rows = match &view.rows {
        Some(Loaded::Ready(rows)) => rows.clone(),
        _ => BTreeMap::new(),
    };
    // The new flashes and tracers join those in play.
    for flash in shots.flashes {
        let model = match flash.shows {
            Flashed::Model(name) => Some(name),
            Flashed::Row(row) => rows.get(&row).cloned(),
        };
        let Some(model) = model else { continue };
        view.next += 1;
        view.live.push(Live {
            model,
            key: view.next,
            kind: LiveKind::Flash {
                by: flash.by,
                hand: flash.hand,
                clip: flash.clip,
                at: flash.at,
                way: flash.way,
                scale: flash.scale,
            },
            since: flash.ms,
        });
    }
    for tracer in shots.tracers {
        view.next += 1;
        view.live.push(Live {
            model: tracer.model,
            key: view.next,
            kind: LiveKind::Tracer {
                from: tracer.from,
                to: tracer.to,
            },
            since: tracer.ms,
        });
    }
    let models = &view.models;
    let clip_ms = |model: &str, clip: &str| match models.get(model) {
        Some(Loaded::Ready(Some(read))) => read
            .model
            .clips
            .get(clip)
            .map_or(FLASH_MS, |clip| f64::from(clip.length) * 1000.0),
        _ => FLASH_MS,
    };
    let to_game = game_to_bevy().inverse();
    let muzzle = |by: SimId, hand: u8| {
        let guns = guns.0.get(&by)?;
        let (root, muzzle) = guns[usize::from(hand.min(1))].or(guns[0])?;
        let place = places.get(root).ok()?;
        Some(to_game * place.transform_point(muzzle))
    };
    let mut shown: Vec<Shown> = shots
        .bolts
        .into_iter()
        .map(|bolt| Shown {
            key: u64::from(bolt.key.0) << 40 ^ bolt.key.1,
            model: bolt.model,
            at: bolt.at,
            way: bolt.way,
            scale: 1.0,
            clip: TRAVEL,
            since: now,
        })
        .collect();
    let mut counts = [shown.len(), 0, 0];
    // A flash lasts its clip; a tracer until it reaches its end.
    view.live.retain(|live| match live.kind {
        LiveKind::Flash { clip, .. } => now < live.since + clip_ms(&live.model, clip),
        LiveKind::Tracer { from, to } => {
            let flown = (now - live.since) as f32 / 1000.0 * TRACER_SPEED;
            flown < from.distance(to)
        }
    });
    for live in view.live.iter().filter(|live| live.since <= now) {
        // Flashes and tracers keyed apart from the bolts.
        let key = live.key | 1 << 63;
        shown.push(match live.kind {
            LiveKind::Flash {
                by,
                hand,
                clip,
                at,
                way,
                scale,
            } => {
                counts[1] += 1;
                Shown {
                    model: live.model.clone(),
                    key,
                    at: muzzle(by, hand).unwrap_or(at),
                    way,
                    scale,
                    clip,
                    since: live.since,
                }
            }
            LiveKind::Tracer { from, to } => {
                counts[2] += 1;
                let flown = (now - live.since) as f32 / 1000.0 * TRACER_SPEED;
                let way = (to - from).normalize_or(Vec3::Y);
                Shown {
                    model: live.model.clone(),
                    key,
                    at: from + way * flown,
                    way,
                    scale: 1.0,
                    clip: TRAVEL,
                    since: live.since,
                }
            }
        });
    }
    for (count, seen) in counts.iter().zip(&mut view.shown) {
        *seen += count;
    }
    // Each in the copy that showed it last, else a free one, else the
    // oldest; made new for a new key, placed and posed.
    let mut placed: Vec<(String, usize, Vec3, f64)> = Vec::new();
    let mut used: BTreeMap<String, Vec<bool>> = BTreeMap::new();
    {
        let mut transforms = posing.p0();
        for item in shown {
            let Some(copies) = view.pools.get_mut(&item.model) else {
                continue;
            };
            let taken = used
                .entry(item.model.clone())
                .or_insert_with(|| vec![false; copies.len()]);
            let Some(k) = pick(copies, taken, item.key) else {
                continue;
            };
            taken[k] = true;
            let pooled = &mut copies[k];
            if pooled.shows.is_none_or(|(key, _)| key != item.key) {
                pooled.spawned.reset();
                pooled.shows = Some((item.key, item.since));
            }
            let since = pooled.shows.map_or(now, |(_, since)| since);
            let copy = &mut pooled.spawned;
            if let Ok(mut transform) = transforms.get_mut(copy.root) {
                *transform = Transform::from_translation(item.at)
                    .with_rotation(crate::minigame_view::heading(item.way))
                    .with_scale(Vec3::splat(item.scale));
            }
            copy.anims = vec![Anim {
                name: item.clip.to_string(),
                looping: false,
                overlay: false,
                since: since as u64,
            }];
            let anims = copy.anims.clone();
            fx_model::animate(copy, &anims, (now, dt), Vec::new(), &mut transforms);
            placed.push((item.model, k, item.at, since));
        }
    }
    let mut drawing = posing.p1();
    let turn = cameras
        .iter()
        .next()
        .and_then(|camera| drawing.transforms.get(camera).ok())
        .map_or(Quat::IDENTITY, |t| t.rotation);
    let facing = fx_model::facing(turn);
    let mut lit = Vec::new();
    for (model, k, at, since) in placed {
        let Some(copy) = view
            .pools
            .get_mut(&model)
            .map(|copies| &mut copies[k].spawned)
        else {
            continue;
        };
        if let Ok(mut visible) = drawing.visibility.get_mut(copy.root) {
            *visible = Visibility::Inherited;
        }
        drawing.effects(copy, None, (now, dt), facing);
        if let Some(Loaded::Ready(Some(read))) = models.get(&model) {
            lit.extend(light(read, copy, at, ((now - since) / 1000.0) as f32));
        }
    }
    // The copies not shown: hidden, their particles cleared when shown next
    // (`Spawned::reset`).
    for (name, copies) in &mut view.pools {
        let taken = used.get(name);
        for (k, pooled) in copies.iter_mut().enumerate() {
            if taken.is_some_and(|taken| taken[k]) || pooled.shows.is_none() {
                continue;
            }
            pooled.shows = None;
            pooled.spawned.reset();
            pooled.spawned.anims.clear();
            // Hidden whole, its particles with it (their meshes stand apart
            // from its root: left shown, a bolt's streak stays where the
            // bolt stopped, in a wall or a body); drawing its effects again
            // with nothing playing would key the model's idle clip on it,
            // and the next shot it shows would draw nothing.
            pooled.spawned.hide(&mut drawing.visibility);
        }
    }
    lights.set_if_neq(ShotLights(lit));
    facts.0.insert(
        "blaster_view".into(),
        serde_json::json!({
            "bolts_shown": view.shown[0],
            "flashes_shown": view.shown[1],
            "tracers_shown": view.shown[2],
            "models": view.models.keys().collect::<Vec<_>>(),
            "pools": view.pools.len(),
            "live": view.live.len(),
            "lights": lights.0.len(),
        }),
    );
}

/// The light of `read`'s model on `copy` at `at` (game space), `seconds`
/// into its clip, as its clip keys its colour; none once out.
fn light(read: &Read, copy: &Spawned, at: Vec3, seconds: f32) -> Option<GpuLight> {
    let ir = &copy.model.placed.ir;
    let light = ir.lights.first()?;
    let colour = colour_at(&read.light, seconds).unwrap_or(Vec3::from_array(light.color));
    if colour.max_element() <= 0.01 {
        return None;
    }
    Some(GpuLight {
        position_radius: (game_to_bevy() * at).extend(light.radius),
        color_multiplier: colour.extend(light.multiplier),
        kind: UVec4::new(u32::from(light.ambient_only), light.dynamic_type, 0, 0),
    })
}
