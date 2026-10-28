//! The fight as JA draws it, over `cast`'s characters, from the presented
//! snapshots alone:
//!
//! - Bolts: a bolt in flight is drawn where it is (`Missile::at`, at the
//!   moment drawn), along its velocity, as KOTOR's executable draws its
//!   projectile: its weapon's `ammunitiontypes.2da` row's model (a Power
//!   Blast's `model1`), drawn by `blaster_view`.
//!   The sim's muzzle is JA's, by the eye (`CalcMuzzlePoint`), which on a
//!   KOTOR body is behind the gun held out: a bolt is drawn leaving the gun
//!   as drawn (its model's `bullethook`, `cast::Guns`), off its way by the
//!   difference and flying straight to its way `CONVERGE` units out, where
//!   the barrel aims (QUIRKS.md Q-0085, a fixed divergence; where it hits
//!   is the sim's).
//! - Hits: an event new to a character's ring (by sequence number) struck
//!   by a blaster (the event's weapon) plays KOTOR's blaster impact
//!   (`BOLT_HIT`, drawn by `script_fx`) and the weapon's impact sound
//!   (`impactsound0`, `weapon_sounds`) at the hit. An event names the
//!   location, not the point: the effect goes on the body's node for that
//!   location (`cast::Hitboxes`), or the chest, `SURFACE` units out toward
//!   the attacker. Hits that drew no blood (refused: an ally's stray shot)
//!   have none, nor have parried and deflected ones, powers', blasts', or
//!   unarmed and baton hits.
//! - Blade hits: a saber's or a melee blade's plays KOTOR's lightsaber
//!   sparks (`HIT_SPARKS`, drawn by `script_fx`) at the hit as above, and
//!   its hit sound there: a lightsaber's one of JA's three
//!   (`sound/weapons/saber/saberhit1–3`, by the hit's numbers), any other
//!   weapon's KOTOR's for what the struck wears (`weapon_sounds`). Fists',
//!   claws' and batons' hits sound so too, with no effect.
//! - Blocks: an event of a blade parried (a block by state, a clash, a
//!   thrown saber turned) plays KOTOR's parry and clash sparks
//!   (`PARRY_SPARKS`, `CLASH_SPARKS`) where it happened (the event's
//!   point); a clash marks both blades, and plays once. Its sound and the clash flare are `blade_fx`'s. A bolt
//!   turned plays KOTOR's deflection (`BOLT_DEFLECTED`) where it was
//!   turned, and the blaster's parry sound.
//! - Contact sparks (I22 C20): a joined player's own blade meeting a body
//!   as its client draws it (`play_view`, by `sim::seen_cuts`) plays
//!   KOTOR's hit sparks at once, where it met; the authority's hit, when it comes,
//!   plays none again. One that never comes leaves only the sparks: no
//!   flinch, no numbers, those wait for the snapshot.
//! - Muzzle flashes and own bolts (`shots`): each shot's muzzle flash
//!   (the row's `muzzleflash`) on the gun's muzzle as drawn, and its feat's
//!   visual there (`blaster_view::feat_row`), and its weapon's shot sound
//!   (`shotsound0`, a Power Blast's `shotsound1`) in the shooter's weapon
//!   slot, going with it; a joined player's own shots and bolts from its
//!   predictions at once, its bolts handed over to the host's copies.
//! - Death cries: a creature that falls cries its sound set's death line
//!   (`viewer::CreatureView::cry`) on its voice, where it falls (`cries`).
//! - Saber marks: the burns and glows blades leave along walls
//!   (`saber_marks`, cut by `blade_fx`).
//! - Other views' effects (`Bursts`, `Added`) are drawn with the hits.
//!   What effects spawn is settled (`Drawn::take`): their sounds asked for
//!   (`sound::Cues`), their runners' effects played, their decals laid as
//!   marks (`docs/FX-PRIMITIVES.md`); a hit's on a body lays none (JA's
//!   decals go onto the level's faces only).
//!
//! Primitives are drawn as `scene_fx` draws them, one mesh per shader.
//! Their random numbers come from the bolt or the hit and the client frame,
//! so a held shot (`anim`) repeats.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use asset_ja::fx::{self, Rng, Spawned};
use asset_odyssey::texture::TextureIr;
use bevy::prelude::*;
use collision::{Contents, Extents};
use console::DumpFacts;
use sim::combat::specials::Special;
use sim::combat::{Delivery, Event, Missile, Outcome, mind, weapons};
use sim::module::ObjectKind;
use sim::{EntityState, Instances, Kind, SimId, Snapshot};

use crate::blaster_view::{BlasterShots, BoltShown, Flash, Flashed};
use crate::cast::{self, Guns, Hitboxes};
use crate::lifecycle_view::SceneCamera;
use crate::material::OdysseyMaterial;
use crate::saber_marks::{self, Cuts};
use crate::scene::{SceneContent, game_to_bevy};
use crate::scene_fx::{self, Batch, ShaderMeshes};
use crate::script_fx::{HookVisual, HookVisuals, Place};
use crate::shots::{self, Fate, Handover, OwnBolt, OwnFire, Shot};
use crate::sim_host::{PresentedSnapshot, SceneSwap, view_ms};
use crate::sound::{Cue, Cues};
use crate::viewer::EffectBlend;
use crate::viewer_ja::Files;
use crate::weapon_sounds::{Met, WeaponSounds};

const UNIT: f32 = asset_ja::UNIT;
/// JA's client frames, as the effects are timed (QUIRKS.md Q-0005).
const FRAME_MS: f32 = 1000.0 / 60.0;
/// How far out of a hit location's node its effect plays (units): about a
/// KOTOR body's half depth (tuning; JA plays it on the trace's surface).
const SURFACE: f32 = 6.0;
/// A shot's sound's gain.
const FIRE_GAIN: f32 = 1.0;
/// How far a muzzle flash plays past the gun hand (units) for a gun model
/// without a muzzle: about a blaster's length (tuning).
const GUN: f32 = 8.0;
/// How far a bolt flies (units) before it is drawn on its own way rather
/// than from the gun (`Path::launch`), and how far along its way the gun is
/// aimed: about 6.5 m, the barrel then a few degrees off the view.
pub const CONVERGE: f32 = 256.0;
/// Each shot's fire sound's playback rate, and its share of `FIRE_GAIN`,
/// drawn from these (a blaster has one fire sound: the same one, unvaried,
/// wears).
const FIRE_RATE: (f32, f32) = (0.92, 1.08);
const FIRE_SHARE: (f32, f32) = (0.8, 1.0);
/// The most impact primitives kept alive; the oldest go first.
const MOST_PRIMITIVES: usize = 4096;
/// Marks laid at once (the renderer's `r_markcount`), and how long one
/// pushed out takes to fade (ms).
const MOST_MARKS: usize = 100;
const MARK_FADE_MS: f32 = 1000.0;
/// The most effects runners start for one settling (a runner that runs
/// itself stops there).
const MOST_RUNS: usize = 64;
/// How deep runners' effects are read.
const RUNNER_DEPTH: usize = 3;

/// KOTOR's sparks (`visualeffects.2da` rows, drawn by `script_fx`), as its
/// executable plays them: a blade's hit on a body `VFX_COM_SPARKS_LIGHTSABER`
/// (`v_spkb_imp`), a parry `VFX_COM_SPARKS_PARRY_METAL` (`v_parrspr_imp`)
/// and a clip's `Clash` that with `VFX_COM_SPARKS_LARGE` (`v_spkr_imp`).
/// They play where the blade met (the executable's at the struck's
/// `impact` node, the parrier's saber hook).
const HIT_SPARKS: u16 = 4004;
const PARRY_SPARKS: u16 = crate::clip_events::CONTACT_SPARKS;
const CLASH_SPARKS: u16 = 4003;
/// KOTOR's blaster impact on a body (`VFX_COM_BLASTER_IMPACT`,
/// `v_blaster_imp`) and a bolt's deflection (`VFX_COM_BLASTER_DEFLECTION`,
/// `v_blastdef_imp`), as its executable plays them.
const BOLT_HIT: u16 = 4024;
const BOLT_DEFLECTED: u16 = 4023;
/// A lightsaber's hit on a body sounds one of these (`EV_SABER_HIT`).
const BLADE_HIT_SOUNDS: [&str; 3] = [
    "sound/weapons/saber/saberhit1.wav",
    "sound/weapons/saber/saberhit2.wav",
    "sound/weapons/saber/saberhit3.wav",
];

/// `weaponsounds.2da`'s rows (by label) for a blow whose striker's row is
/// not known: a melee weapon's blades', the rest's unarmed.
const BLADE_ROW: &str = "Blade";
const UNARMED_ROW: &str = "Unarmed";
/// How long a contact spark answers for the authority's hit it foretold
/// (ms of the view's time): past a round trip and the interpolation delay.
const CONTACT_MATCH_MS: f32 = 1000.0;
/// How long a stopped own bolt is remembered (own ms): the host's copy of
/// it, should one show after all, is not drawn.
const KEEP_ENDED_MS: f64 = 1000.0;

/// A joined player's own blade meeting a body as its client draws it (I22
/// C20): `by` cut `victim` at `point` (units).
pub struct Contact {
    pub by: SimId,
    pub victim: SimId,
    pub point: Vec3,
}

/// The contacts found since the last frame (`play_view` adds, this draws).
#[derive(Resource, Default)]
pub struct Contacts(pub Vec<Contact>);

/// An effect another view asks for: by name (one `load` was given, under
/// `effects/`), where (units), facing, at which moment (the view's ms),
/// played how many times at once.
pub struct Burst {
    pub effect: &'static str,
    pub at: Vec3,
    pub normal: Vec3,
    pub ms: f32,
    pub times: u32,
    /// Its whole axis (forward, left, up) where its roll matters (a
    /// power's, along the caster's aim); else one made from `normal`.
    pub axis: Option<[Vec3; 3]>,
}

/// The effects asked for since the last frame.
#[derive(Resource, Default)]
pub struct Bursts(pub Vec<Burst>);

/// Primitives other views made themselves since the last frame (a beam),
/// drawn with the impacts.
#[derive(Resource, Default)]
pub struct Added(pub Vec<Spawned>);

/// The fight's drawing: views that ask for effects run before it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FightDrawn;

/// Every sound the fight asks for itself (`sound::load`; the effects' own
/// are `Setup::sounds`).
pub fn sounds() -> impl Iterator<Item = String> {
    BLADE_HIT_SOUNDS.into_iter().map(String::from)
}

/// The fight's effects, read from JA's files, with their shaders' textures
/// and blends.
pub struct Setup {
    /// The effects other views ask for by name (`Bursts`).
    named: BTreeMap<&'static str, fx::Effect>,
    /// The effects runners play, by name.
    runners: BTreeMap<String, fx::Effect>,
    textures: BTreeMap<String, TextureIr>,
    blends: BTreeMap<String, EffectBlend>,
}

/// Reads `named` for the other views' `Bursts`, with `shaders` no effect
/// names; what is missing or not
/// supported goes to the notes.
pub fn load(
    files: &Files,
    named: &[&'static str],
    shaders: &[&str],
    notes: &mut Vec<String>,
) -> Setup {
    let named: BTreeMap<&'static str, fx::Effect> = named
        .iter()
        .map(|&name| {
            let effect =
                crate::viewer_ja::effect_file(files, &format!("effects/{name}.efx"), notes);
            (name, effect)
        })
        .collect();
    // The effects runners play, read as deep as `RUNNER_DEPTH`.
    let mut runners: BTreeMap<String, fx::Effect> = BTreeMap::new();
    let mut wanted: BTreeSet<String> = named.values().flat_map(runs_of).collect();
    for _ in 0..RUNNER_DEPTH {
        for name in std::mem::take(&mut wanted) {
            if runners.contains_key(&name) {
                continue;
            }
            let effect =
                crate::viewer_ja::effect_file(files, &format!("effects/{name}.efx"), notes);
            wanted.extend(runs_of(&effect));
            runners.insert(name, effect);
        }
    }
    let mut shaders: Vec<String> = named
        .values()
        .chain(runners.values())
        .flat_map(|effect| &effect.templates)
        .flat_map(|template| &template.shaders)
        .map(String::as_str)
        .chain(shaders.iter().copied())
        .map(str::to_ascii_lowercase)
        .collect();
    shaders.sort();
    shaders.dedup();
    let (textures, missing) = crate::viewer_ja::textures(files, &shaders);
    if !missing.is_empty() {
        notes.push(format!(
            "combat effects: no texture for {}",
            missing.join(", ")
        ));
    }
    let blends = crate::viewer_ja::effect_blends(files, shaders, notes);
    Setup {
        named,
        runners,
        textures,
        blends,
    }
}

/// The effects `effect`'s runners play.
fn runs_of(effect: &fx::Effect) -> Vec<String> {
    effect
        .templates
        .iter()
        .filter(|t| t.kind == fx::Kind::Runner)
        .flat_map(|t| t.play.iter().cloned())
        .collect()
}

impl Setup {
    /// Every sound its effects may play.
    pub fn sounds(&self) -> impl Iterator<Item = &str> {
        self.named
            .values()
            .chain(self.runners.values())
            .flat_map(|effect| &effect.templates)
            .flat_map(|template| &template.sounds)
            .map(String::as_str)
    }
}

/// Its systems; a session comes in with `install`.
pub struct CombatFxPlugin;

/// A session's effects, put into `app`: their meshes are made on the next
/// frame.
pub fn install(app: &mut World, setup: Setup) {
    app.insert_resource(Pending(Some(setup)));
}

/// The effects and the meshes they are drawn into, and what the hits left.
#[derive(Resource)]
struct Drawn {
    named: BTreeMap<&'static str, fx::Effect>,
    runners: BTreeMap<String, fx::Effect>,
    /// The marks laid, oldest first, each with when it began to fade.
    marks: std::collections::VecDeque<(Spawned, Option<f32>)>,
    /// The blades' burns and glows along walls.
    saber_marks: saber_marks::Marks,
    /// The blocks played: a clash marks both blades, and plays once (by
    /// its tick and point).
    blocked: Vec<(u64, [i32; 3])>,
    /// Blocks played: a blade's, a bolt's.
    blocks_seen: [usize; 2],
    /// The contact sparks and own bolts' impacts shown, awaiting the hits
    /// they foretold: by whom, on whom, when (the view's ms).
    shown: Vec<(SimId, SimId, f32)>,
    /// A joined player's own bolts (`shots`), the host's copies that took
    /// over from them, and those of own bolts that had stopped (not drawn).
    own_bolts: Vec<OwnBolt>,
    handovers: BTreeMap<SimId, Handover>,
    hidden: BTreeSet<SimId>,
    /// The last snapshot the view had, to find the shots in the next.
    last: Option<Arc<Snapshot>>,
    /// Muzzle flashes waiting for their shot's moment (the view's ms).
    flashing: Vec<(Shot, f32)>,
    /// Each bolt's launch (`Path::launch`), by shooter and when it was
    /// fired: taken once, when the view reaches the shot, so the arm's kick
    /// after does not shake it; kept while the bolt flies.
    launches: BTreeMap<(SimId, u64), Vec3>,
    shaders: ShaderMeshes,
    /// The hits' primitives, oldest first (the oldest go past
    /// `MOST_PRIMITIVES`).
    impacts: Vec<Spawned>,
    /// Each character's next event number, as far as the view has seen.
    seen: BTreeMap<SimId, u16>,
    /// The scene swap the view was built for.
    generation: u32,
    /// The hits played: a blaster's, a blade's.
    hits: [usize; 2],
    contacts: Tally,
    /// Muzzle flashes played: own (predicted), others'.
    flashes: [usize; 2],
    /// Physics particles' traces through the level (`fx::Spawned::collide`):
    /// in all, the most in one frame, and the longest frame of them (ms).
    physics: (usize, usize, f64),
    /// Own bolts: drawn from the predictions, handed over to the host's
    /// copy, stopped on a body, faded unfired, stopped otherwise (a wall).
    own: [usize; 5],
}

/// The contact sparks shown, and of them those the authority's hit
/// confirmed and those none confirmed in `CONTACT_MATCH_MS`.
#[derive(Default)]
struct Tally {
    shown: usize,
    confirmed: usize,
    unconfirmed: usize,
}

/// The setup until the first frame builds the meshes.
#[derive(Resource)]
struct Pending(Option<Setup>);

impl Plugin for CombatFxPlugin {
    fn build(&self, app: &mut App) {
        // Empty until a session is installed; the blades' and shots'
        // effects read them meanwhile.
        app.init_resource::<Contacts>()
            .init_resource::<OwnFire>()
            .init_resource::<Cues>()
            .init_resource::<Bursts>()
            .init_resource::<Added>()
            .init_resource::<Cuts>()
            .add_systems(PreUpdate, spawn.run_if(resource_exists::<Pending>))
            .add_systems(
                PostUpdate,
                // Before the meshes' changes are sent on (`scene_fx`).
                draw.run_if(resource_exists::<Drawn>)
                    .in_set(FightDrawn)
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::asset::AssetEventSystems),
            )
            .add_systems(PostUpdate, cries.in_set(FightDrawn));
    }
}

/// The creatures alive in the last snapshot seen (in the scene swap
/// `generation`), and the death lines put in the sound bank.
#[derive(Default)]
struct Cried {
    generation: u32,
    alive: BTreeSet<SimId>,
    banked: BTreeSet<String>,
}

/// A creature alive in one snapshot and dead in the next cries its death
/// line there, on its voice, going with it.
fn cries(
    presented: Option<Res<PresentedSnapshot>>,
    swap: Option<Res<SceneSwap>>,
    scene: Option<Res<SceneContent>>,
    mut cried: Local<Cried>,
    mut cues: ResMut<Cues>,
    mut bank: ResMut<crate::sound::Added>,
) {
    let (Some(presented), Some(swap), Some(scene)) = (presented, swap, scene) else {
        return;
    };
    if cried.generation != swap.generation {
        cried.generation = swap.generation;
        cried.alive.clear();
    }
    for snapshot in &presented.1 {
        for entity in &snapshot.entities {
            let (Kind::Object(ObjectKind::Creature, index), Some(fighter)) =
                (entity.kind, entity.fighter)
            else {
                continue;
            };
            if fighter.alive() {
                cried.alive.insert(entity.id);
                continue;
            }
            if !cried.alive.remove(&entity.id) {
                continue;
            }
            let Some((name, clip)) = scene
                .0
                .creature_views
                .iter()
                .find(|view| view.object == index)
                .and_then(|view| view.cry.as_ref())
            else {
                continue;
            };
            let cue = format!("cry/{name}");
            if cried.banked.insert(name.clone()) {
                bank.0.push((cue.clone(), clip.clone()));
            }
            cues.0.push(Cue {
                entity: Some(entity.id),
                channel: audio::Channel::Voice,
                follows: true,
                ..Cue::at(
                    cue,
                    Vec3::from_array(entity.position) / UNIT,
                    snapshot.time_ms as f32,
                )
            });
        }
    }
}

/// A mesh and a material per shader; they outlive the scenes.
fn spawn(
    mut commands: Commands,
    mut pending: ResMut<Pending>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
) {
    let Some(setup) = pending.0.take() else {
        return;
    };
    crate::body_marks::install(&mut commands, &setup.textures, &mut images);
    let shaders = ShaderMeshes::spawn(
        &mut commands,
        (&setup.blends, &setup.textures),
        &mut images,
        &mut meshes,
        &mut materials,
    );
    commands.insert_resource(Drawn {
        named: setup.named,
        runners: setup.runners,
        marks: std::collections::VecDeque::new(),
        saber_marks: saber_marks::Marks::default(),
        blocked: Vec::new(),
        blocks_seen: [0; 2],
        shown: Vec::new(),
        own_bolts: Vec::new(),
        handovers: BTreeMap::new(),
        hidden: BTreeSet::new(),
        last: None,
        flashing: Vec::new(),
        launches: BTreeMap::new(),
        shaders,
        impacts: Vec::new(),
        seen: BTreeMap::new(),
        generation: 0,
        hits: [0; 2],
        contacts: Tally::default(),
        flashes: [0; 2],
        physics: (0, 0, 0.0),
        own: [0; 5],
    });
    commands.remove_resource::<Pending>();
}

impl Drawn {
    /// Whether contact sparks already showed `by`'s hit on `victim` (in
    /// the last `CONTACT_MATCH_MS`); the one that did answers for it.
    fn foretold(&mut self, by: Option<SimId>, victim: SimId) -> bool {
        let found = self
            .shown
            .iter()
            .position(|&(who, whom, _)| Some(who) == by && whom == victim);
        let Some(at) = found else {
            return false;
        };
        self.shown.remove(at);
        self.contacts.confirmed += 1;
        true
    }

    /// `by`'s blow or shot on `victim` was turned: a contact or impact shown
    /// for it foretold wrong.
    fn refuted(&mut self, by: Option<SimId>, victim: SimId) {
        if let Some(at) = self
            .shown
            .iter()
            .position(|&(who, whom, _)| Some(who) == by && whom == victim)
        {
            self.shown.remove(at);
            self.contacts.unconfirmed += 1;
        }
    }

    /// Settles what effects spawned: their sounds asked for where they
    /// are (nobody's, in no slot, as the game plays an effect's), their
    /// runners' effects played in their place (settled in turn), their
    /// decals laid as marks, the rest drawn. An effect played on a body
    /// (`on_body`, a hit's) lays no decal: JA projects one onto the level's
    /// faces only, never onto a model, so the scorch a
    /// bolt's hit carries is a wall's.
    fn take(&mut self, spawned: Vec<Spawned>, cues: &mut Vec<Cue>, on_body: bool) {
        let mut queue: std::collections::VecDeque<Spawned> = spawned.into();
        let mut runs = 0;
        while let Some(spawned) = queue.pop_front() {
            match spawned.kind {
                fx::Kind::Sound => {
                    cues.push(Cue::at(spawned.shader, spawned.origin, spawned.start_ms))
                }
                fx::Kind::Runner if runs < MOST_RUNS => {
                    let Some(effect) = self.runners.get(&spawned.shader) else {
                        continue;
                    };
                    let mut rng = seed(spawned.seed as u32, spawned.start_ms as u32);
                    let axis = fx::axis(spawned.normal);
                    let at = (spawned.origin, spawned.start_ms);
                    queue.extend(fx::play(effect, at.0, axis, at.1, &mut rng, &|_, to| to));
                    runs += 1;
                }
                fx::Kind::Runner => {}
                fx::Kind::Decal if on_body => {}
                fx::Kind::Decal => self.lay(spawned),
                _ => self.impacts.push(spawned),
            }
        }
    }

    /// A mark laid; past `MOST_MARKS` the oldest begins to fade.
    fn lay(&mut self, mark: Spawned) {
        let at = mark.start_ms;
        self.marks.push_back((mark, None));
        let laid = self
            .marks
            .iter()
            .filter(|(_, fading)| fading.is_none())
            .count();
        if laid > MOST_MARKS
            && let Some(oldest) = self.marks.iter_mut().find(|(_, fading)| fading.is_none())
        {
            oldest.1 = Some(at);
        }
    }

    /// The oldest impacts go past `MOST_PRIMITIVES`.
    fn trim(&mut self) {
        let over = self.impacts.len().saturating_sub(MOST_PRIMITIVES);
        self.impacts.drain(..over);
    }
}

/// A repeatable seed from two numbers.
fn seed(a: u32, b: u32) -> Rng {
    crate::view_rng::ja_seed(a, b.wrapping_mul(0x85eb_ca6b))
}

#[allow(clippy::too_many_arguments)]
fn draw(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    (hitboxes, guns, eyes): (Res<Hitboxes>, Res<Guns>, Res<crate::first_person::Eyes>),
    drawn: Option<ResMut<Drawn>>,
    (mut contacts, mut fire): (ResMut<Contacts>, ResMut<OwnFire>),
    (mut cues, mut asked, mut added, mut cuts): (
        ResMut<Cues>,
        ResMut<Bursts>,
        ResMut<Added>,
        ResMut<Cuts>,
    ),
    places: Query<&GlobalTransform>,
    cameras: Query<&GlobalTransform, SceneCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut facts: ResMut<DumpFacts>,
    (kotor, visuals, mut blasters): (
        Option<Res<WeaponSounds>>,
        Option<ResMut<HookVisuals>>,
        ResMut<BlasterShots>,
    ),
) {
    let Some(mut drawn) = drawn else { return };
    // KOTOR's sparks are drawn by `script_fx`; none without it.
    let mut sparked = Vec::new();
    let kotor = kotor.as_deref();
    let drawn = &mut *drawn;
    let cues = &mut cues.0;
    if swap.generation != drawn.generation {
        drawn.generation = swap.generation;
        drawn.impacts.clear();
        drawn.seen.clear();
        drawn.marks.clear();
        drawn.saber_marks.clear();
        cuts.0.clear();
        asked.0.clear();
        added.0.clear();
        drawn.shown.clear();
        drawn.own_bolts.clear();
        drawn.handovers.clear();
        drawn.hidden.clear();
        drawn.last = None;
        drawn.flashing.clear();
        drawn.launches.clear();
        contacts.0.clear();
        fire.shots.clear();
        fire.bolts.clear();
    }
    let current = presented.0.current();
    let now = view_ms(&presented.0) as f32;
    let shown = drawn.shown.len();
    drawn.shown.retain(|&(.., at)| now - at <= CONTACT_MATCH_MS);
    drawn.contacts.unconfirmed += shown - drawn.shown.len();
    let catalog = swap.level.scripts().map(|library| library.items());
    let met = (
        current,
        &Instances::in_snapshot(&swap.level, current),
        catalog,
    );
    sparks(
        drawn,
        std::mem::take(&mut contacts.0),
        (now, met, kotor),
        (cues, &mut sparked),
    );
    // An effect's end traced from it (`org2fromTrace`) stops at the level.
    let states = swap.level.states_in(current);
    let line = |from: Vec3, to: Vec3| {
        swap.level.with(states.as_deref(), |world| {
            world.trace(
                (from * UNIT).to_array(),
                (to * UNIT).to_array(),
                &Extents::POINT,
                Contents::SOLID,
            )
        })
    };
    let trace = |from: Vec3, to: Vec3| Vec3::from_array(line(from, to).end) / UNIT;
    for burst in asked.0.drain(..) {
        let Some(effect) = drawn.named.get(burst.effect) else {
            continue;
        };
        let mut rng = seed(burst.at.x.to_bits() ^ burst.at.y.to_bits(), burst.ms as u32);
        let axis = burst.axis.unwrap_or_else(|| fx::axis(burst.normal));
        let spawned: Vec<Spawned> = (0..burst.times)
            .flat_map(|_| fx::play(effect, burst.at, axis, burst.ms, &mut rng, &trace))
            .collect();
        drawn.take(spawned, cues, false);
    }
    drawn.impacts.append(&mut added.0);
    drawn.trim();
    // The level's own faces, for the saber marks (units).
    let faces = |min: Vec3, max: Vec3| -> Vec<[Vec3; 3]> {
        let Some(built) = swap.level.collision().level() else {
            return Vec::new();
        };
        let bounds = ((min * UNIT).to_array(), (max * UNIT).to_array());
        built
            .faces_in(bounds, Contents::SOLID, states.as_deref())
            .into_iter()
            .filter(|&(brush, _)| {
                built
                    .brush(brush as usize)
                    .is_some_and(|brush| brush.tag.entity.is_none())
            })
            .map(|(_, corners)| corners.map(|p| Vec3::from_array(p) / UNIT))
            .collect()
    };
    for cut in cuts.0.drain(..) {
        drawn.saber_marks.lay(&cut, &faces);
    }
    // Own time: where the own character is drawn, ahead of the view's.
    let me = fire.me;
    let own_now = fire.shift.map(|shift| f64::from(now) + shift);
    for shot in std::mem::take(&mut fire.shots) {
        if let Some(shift) = fire.shift {
            flash(drawn, &shot, (shot.ms - shift) as f32, 0, (cues, kotor));
        }
    }
    for (tick, missile) in std::mem::take(&mut fire.bolts) {
        let known = |b: &OwnBolt| b.tick == tick && b.is(&missile, 0.0);
        if own_now.is_some()
            && !drawn.own_bolts.iter().any(known)
            && let Some(bolt) = OwnBolt::new(tick, &missile)
        {
            drawn.own_bolts.push(bolt);
            drawn.own[0] += 1;
        }
    }
    for snapshot in &presented.1 {
        if let Some(last) = drawn.last.take() {
            for shot in shots::shots(&last, snapshot, &swap.level, me) {
                flash(drawn, &shot, shot.ms as f32, 1, (cues, kotor));
            }
        }
        drawn.last = Some(snapshot.clone());
        if let (Some(me), Some(own_now)) = (me, own_now) {
            let lead = own_now - f64::from(now);
            for bolt in &mut drawn.own_bolts {
                if bolt.judge(snapshot, me, (own_now, lead)) {
                    drawn.own[3] += 1;
                }
            }
        }
        hits(
            drawn,
            snapshot,
            (&swap.level, kotor),
            &hitboxes,
            &places,
            (cues, &mut sparked),
        );
    }
    if let (Some(me), Some(own_now)) = (me, own_now) {
        fly(
            drawn,
            (me, own_now, now),
            (&swap.level, kotor),
            current,
            &fire.bodies,
            (cues, &mut sparked),
        );
    }
    place_flashes(
        drawn,
        (now, kotor),
        (&hitboxes, &guns, eyes.0),
        &places,
        &mut blasters.flashes,
    );
    drawn.impacts.retain(|p| p.end_ms + FRAME_MS >= now);
    // Physics particles bounce off the level, a trace a frame each.
    let started = std::time::Instant::now();
    let meets = |from: Vec3, to: Vec3| {
        let met = line(from, to);
        (met.fraction < 1.0 && !met.start_solid).then(|| {
            (
                Vec3::from_array(met.end) / UNIT,
                Vec3::from_array(met.plane.normal),
            )
        })
    };
    let traces: usize = drawn
        .impacts
        .iter_mut()
        .map(|primitive| primitive.collide(now, FRAME_MS, &meets))
        .sum();
    let physics = &mut drawn.physics;
    *physics = (
        physics.0 + traces,
        physics.1.max(traces),
        physics.2.max(started.elapsed().as_secs_f64() * 1000.0),
    );
    // Each bolt leaves the gun as drawn when the view reaches its shot;
    // a launch goes with its bolt.
    let mut flying = BTreeSet::new();
    for missile in current.entities.iter().filter_map(|e| e.missile) {
        if missile.lob.is_none() {
            let shot = (missile.owner, missile.from_ms);
            flying.insert(shot);
            if now >= missile.from_ms as f32 {
                let base = Vec3::from_array(missile.base);
                launch(&mut drawn.launches, (&guns, &places), shot, base);
            }
        }
    }
    if let (Some(me), Some(own_now)) = (me, own_now) {
        for own in &drawn.own_bolts {
            let shot = (me, own.fired as u64);
            flying.insert(shot);
            if own_now >= own.fired {
                launch(&mut drawn.launches, (&guns, &places), shot, own.base);
            }
        }
    }
    drawn.launches.retain(|shot, _| flying.contains(shot));
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    let eye = scene_fx::eye_of(camera);
    let to_bevy = |p: Vec3| game_to_bevy() * (p * UNIT);
    // The host's copies of own bolts: each drawn as far off as the own bolt
    // was when it took over, closing; none for one that stopped.
    let mut offsets: BTreeMap<SimId, Option<Vec3>> = BTreeMap::new();
    if let (Some(me), Some(own_now)) = (me, own_now) {
        for entity in &current.entities {
            let Some(missile) = entity
                .missile
                .filter(|m| m.owner == me && m.lob.is_none() && m.held.is_none())
            else {
                continue;
            };
            let offset = match drawn.handovers.get(&entity.id) {
                Some(handover) => Some(handover.left(now)),
                None if drawn.hidden.contains(&entity.id) => None,
                None => take_over(drawn, entity.id, &missile, (own_now, now)),
            };
            if offset.is_none() {
                drawn.hidden.insert(entity.id);
            }
            offsets.insert(entity.id, offset);
        }
    }
    let mut batches: BTreeMap<String, Batch> = BTreeMap::new();
    let shaders = &drawn.shaders;
    let mut add = |spawned: &Spawned, rng: &mut Rng, at: f32| {
        let shader = spawned.shader.to_ascii_lowercase();
        if let Some(look) = shaders.look(&shader) {
            let batch = batches.entry(shader).or_default();
            scene_fx::primitive(batch, spawned, look, (at, FRAME_MS), rng, &eye, to_bevy);
        }
    };
    let mut bolts = 0;
    let objects = Instances::in_snapshot(&swap.level, current);
    for entity in &current.entities {
        let Some(missile) = entity.missile else {
            continue;
        };
        // A grenade is drawn by `grenade_view`.
        if missile.lob.is_some() {
            continue;
        }
        let Delivery::Blaster(_) = missile.weapon.delivery else {
            continue;
        };
        // A bolt held on a body for its grace has met it: not drawn.
        if missile.held.is_some() {
            continue;
        }
        let offset = match offsets.get(&entity.id) {
            Some(None) => continue,
            Some(Some(offset)) => *offset,
            None => Vec3::ZERO,
        };
        let shooter = cast::entity(current, missile.owner);
        let Some(model) = bolt_model(kotor, &missile.weapon, (shooter, &objects)) else {
            continue;
        };
        let path = Path {
            base: Vec3::from_array(missile.base) + offset,
            velocity: Vec3::from_array(missile.velocity),
            fired: missile.from_ms as f32,
            until: f32::INFINITY,
            launch: launched(drawn, missile.owner, missile.from_ms),
        };
        if let Some(shown) = path.shown((entity.id.0, 0), model, now) {
            blasters.bolts.push(shown);
            bolts += 1;
        }
    }
    // The own bolts not yet handed over, at own time.
    if let (Some(own_now), Some(me)) = (own_now, me) {
        let shooter = cast::entity(current, me);
        for own in &drawn.own_bolts {
            let Some(model) = bolt_model(kotor, &own.weapon, (shooter, &objects)) else {
                continue;
            };
            let path = Path {
                base: own.base,
                velocity: own.velocity,
                fired: own.fired as f32,
                until: match own.fate {
                    Fate::Ended(end) => end as f32,
                    Fate::Handed(_) => continue,
                    _ => f32::INFINITY,
                },
                launch: launched(drawn, me, own.fired as u64),
            };
            let key = (u32::MAX - me.0, own.tick);
            if let Some(shown) = path.shown(key, model, own_now as f32) {
                blasters.bolts.push(shown);
            }
        }
    }
    for spawned in drawn.impacts.iter().filter(|spawned| spawned.alive(now)) {
        let mut rng = seed(spawned.seed as u32, (now / FRAME_MS) as u32);
        add(spawned, &mut rng, now);
    }
    // The marks: their start colour and alpha, fading once pushed out.
    drawn
        .marks
        .retain(|(_, fading)| fading.is_none_or(|from| now < from + MARK_FADE_MS));
    for (mark, fading) in &drawn.marks {
        let shader = mark.shader.to_ascii_lowercase();
        let Some(blend) = drawn.shaders.blend(&shader) else {
            continue;
        };
        if now < mark.start_ms {
            continue;
        }
        let fade = fading.map_or(1.0, |from| 1.0 - (now - from) / MARK_FADE_MS);
        let darken = if matches!(blend, EffectBlend::Add) {
            fade
        } else {
            1.0
        };
        let byte = |v: f32| (v * 255.0).clamp(0.0, 255.0) as u8;
        let rgb = mark.rgb_start * darken;
        let rgba = [
            byte(rgb.x),
            byte(rgb.y),
            byte(rgb.z),
            byte(mark.alpha[0] * fade),
        ];
        batches.entry(shader).or_default().quad(
            fx::mark(mark),
            scene_fx::vertex_colour(rgba, blend),
            to_bevy,
        );
    }
    drawn
        .saber_marks
        .draw(now, &drawn.shaders, &mut batches, to_bevy);
    drawn.shaders.fill(batches, &mut meshes);
    let flying: BTreeSet<SimId> = current.entities.iter().map(|e| e.id).collect();
    drawn.handovers.retain(|id, _| flying.contains(id));
    drawn.hidden.retain(|id| flying.contains(id));
    if let Some(own_now) = own_now {
        drawn.own_bolts.retain(|own| match own.fate {
            Fate::Handed(_) => false,
            // Kept a while: a host's copy that shows late is not drawn.
            Fate::Ended(end) => own_now - end < KEEP_ENDED_MS,
            _ => own_now - own.fired < weapons::BOLT_LIFE_MS as f64,
        });
    } else {
        drawn.own_bolts.clear();
    }
    if let Some(mut visuals) = visuals {
        visuals.0.extend(sparked);
    }
    facts.0.insert(
        "combat_fx".into(),
        serde_json::json!({
            "bolts_drawn": bolts,
            "impact_primitives": drawn.impacts.len(),
            "marks": drawn.marks.len(),
            "saber_marks": drawn.saber_marks.len(),
            "physics_traces": drawn.physics.0,
            "physics_traces_most_in_a_frame": drawn.physics.1,
            "physics_ms_worst_frame": drawn.physics.2,
            "bolt_hits_seen": drawn.hits[0],
            "blade_hits_seen": drawn.hits[1],
            "contacts_shown": drawn.contacts.shown,
            "contacts_confirmed": drawn.contacts.confirmed,
            "contacts_unconfirmed": drawn.contacts.unconfirmed,
            "flashes_own": drawn.flashes[0],
            "flashes_others": drawn.flashes[1],
            "own_bolts": drawn.own[0],
            "own_bolts_handed": drawn.own[1],
            "own_bolts_met": drawn.own[2],
            "own_bolts_unfired": drawn.own[3],
            "own_bolts_stopped": drawn.own[4],
            "blade_blocks_seen": drawn.blocks_seen[0],
            "bolt_blocks_seen": drawn.blocks_seen[1],
        }),
    );
}

/// A bolt's way: from `base` (units) at `fired` (ms) at `velocity`
/// (units per second), stopped at `until`; drawn leaving the gun, `launch`
/// off its way at first, closing over its first `CONVERGE` units.
struct Path {
    base: Vec3,
    velocity: Vec3,
    fired: f32,
    until: f32,
    launch: Vec3,
}

impl Path {
    /// How far off its way the bolt is drawn `ms` after it was fired: all
    /// of `launch` at first, none once `CONVERGE` units out, in step with
    /// the way flown, so it flies straight from the muzzle to its way there,
    /// along the barrel aimed at that point (`cast::gun_aim`).
    fn off(&self, ms: f32) -> Vec3 {
        let gone = self.velocity.length() * ms / 1000.0;
        self.launch * (1.0 - gone / CONVERGE).clamp(0.0, 1.0)
    }

    /// The bolt `key` in `model` as drawn at `now`: none before it was
    /// fired, nor once it stopped (KOTOR's bolt goes as it arrives).
    fn shown(&self, key: (u32, u64), model: String, now: f32) -> Option<BoltShown> {
        if now < self.fired || now > self.until {
            return None;
        }
        let t = now - self.fired;
        let at = self.base + self.velocity * (t / 1000.0) + self.off(t);
        // Its way as drawn: toward where it will be a moment on.
        let next = self.base + self.velocity * ((t + FRAME_MS) / 1000.0) + self.off(t + FRAME_MS);
        Some(BoltShown {
            key,
            model,
            at: at * UNIT,
            way: (next - at).normalize_or(self.velocity.normalize_or(Vec3::Y)),
        })
    }
}

/// The model a bolt fired with `weapon` is drawn as: its row's `model0`,
/// a Power Blast's `model1` (its shot carries more damage than the gun in
/// its shooter's hand).
fn bolt_model(
    kotor: Option<&WeaponSounds>,
    weapon: &sim::combat::sheet::Weapon,
    (shooter, objects): (Option<&EntityState>, &Instances),
) -> Option<String> {
    let ammo = kotor?.ammo(weapon.base_item)?;
    let held = shooter.map(|shooter| cast::weapon(shooter, objects));
    let power = held.is_some_and(|held| weapon.damage_bonus > held.damage_bonus);
    ammo.models[usize::from(power)]
        .clone()
        .or_else(|| ammo.models[0].clone())
}

/// Where `who`'s gun in `hand` (`Guns`) as drawn lets its shots out
/// (units), if it holds one with a muzzle.
pub(crate) fn gun_muzzle(
    guns: &Guns,
    places: &Query<&GlobalTransform>,
    (who, hand): (SimId, u8),
) -> Option<Vec3> {
    let guns = guns.0.get(&who)?;
    let (root, muzzle) = guns[usize::from(hand.min(1))].or(guns[0])?;
    let place = places.get(root).ok()?;
    Some(game_to_bevy().inverse() * place.transform_point(muzzle) / UNIT)
}

/// The muzzle of `who`'s guns as drawn nearest `near` (units): the one a
/// bolt that left the sim's muzzle there was fired from.
pub(crate) fn nearest_muzzle(
    guns: &Guns,
    places: &Query<&GlobalTransform>,
    who: SimId,
    near: Vec3,
) -> Option<Vec3> {
    [0, 1]
        .into_iter()
        .filter_map(|hand| gun_muzzle(guns, places, (who, hand)))
        .min_by(|a, b| {
            a.distance_squared(near)
                .total_cmp(&b.distance_squared(near))
        })
}

/// Takes the launch of `who`'s bolt fired at `fired` from the sim's muzzle
/// `base` (units), once: from where the gun is drawn to `base`; none
/// without a gun to leave.
fn launch(
    launches: &mut BTreeMap<(SimId, u64), Vec3>,
    (guns, places): (&Guns, &Query<&GlobalTransform>),
    (who, fired): (SimId, u64),
    base: Vec3,
) {
    launches.entry((who, fired)).or_insert_with(|| {
        nearest_muzzle(guns, places, who, base).map_or(Vec3::ZERO, |gun| gun - base)
    });
}

/// The launch taken for `who`'s bolt fired at `fired` (`launch`).
fn launched(drawn: &Drawn, who: SimId, fired: u64) -> Vec3 {
    drawn
        .launches
        .get(&(who, fired))
        .copied()
        .unwrap_or(Vec3::ZERO)
}

/// A muzzle flash for `shot` at `start` (the view's ms), and its shot
/// sound then, each shot's a little higher or lower and louder or softer
/// than the last (`FIRE_GAIN`, `FIRE_RATE`, `FIRE_SHARE`); `whose` 0 for the
/// own, 1 for the others'. The flash waits for its moment to be placed on
/// the gun as drawn then.
fn flash(
    drawn: &mut Drawn,
    shot: &Shot,
    start: f32,
    whose: usize,
    (cues, kotor): (&mut Vec<Cue>, Option<&WeaponSounds>),
) {
    let power = matches!(shot.special, Some(Special::PowerBlast(_)));
    if let Some(sound) = kotor.and_then(|kotor| kotor.shot(shot.base_item, power)) {
        let mut rng = seed(shot.by.0, start.to_bits());
        cues.push(Cue {
            entity: Some(shot.by),
            channel: audio::Channel::Weapon,
            follows: true,
            rate: f64::from(rng.range(FIRE_RATE.0, FIRE_RATE.1)),
            gain: FIRE_GAIN * rng.range(FIRE_SHARE.0, FIRE_SHARE.1),
            ..Cue::at(sound, shot.muzzle, start)
        });
    }
    drawn.flashing.push((*shot, start));
    drawn.flashes[whose] += 1;
}

/// The muzzle flashes whose moment came, asked of `blaster_view` from this
/// frame: the row's muzzle flash and the shot's feat's visual, on the
/// shooter's gun's muzzle as drawn (where the executable hangs them, on
/// its `bullethook`); for a gun not drawn, `GUN` units on from the gun hand
/// along the shot; else at the sim's muzzle. A flash before the eyes the
/// camera looks through (`eyes`) plays smaller.
fn place_flashes(
    drawn: &mut Drawn,
    (now, kotor): (f32, Option<&WeaponSounds>),
    (hitboxes, guns, eyes): (&Hitboxes, &Guns, Option<SimId>),
    places: &Query<&GlobalTransform>,
    out: &mut Vec<Flash>,
) {
    let to_game = game_to_bevy().inverse();
    let due: Vec<(Shot, f32)> = drawn
        .flashing
        .extract_if(.., |(_, start)| *start <= now)
        .collect();
    for (shot, start) in due {
        let hand = || {
            hitboxes
                .0
                .get(&shot.by)
                .and_then(|spots| spots[sim::combat::Location::HandRight as usize])
                .and_then(|node| places.get(node).ok())
                .map(|place| to_game * place.translation() / UNIT + shot.forward * GUN)
        };
        let at = gun_muzzle(guns, places, (shot.by, shot.hand))
            .or_else(hand)
            .unwrap_or(shot.muzzle);
        let scale = if eyes == Some(shot.by) {
            crate::first_person::FLASH_SCALE
        } else {
            1.0
        };
        let muzzle = kotor
            .and_then(|kotor| kotor.ammo(shot.base_item))
            .and_then(|ammo| ammo.muzzle_flash.clone())
            .map(Flashed::Model);
        let feat = shot
            .special
            .and_then(crate::blaster_view::feat_row)
            .map(Flashed::Row);
        out.extend(muzzle.into_iter().chain(feat).map(|shows| Flash {
            by: shot.by,
            hand: shot.hand,
            shows,
            clip: crate::blaster_view::IMPACT,
            at: at * UNIT,
            way: shot.forward,
            ms: f64::from(start.max(now)),
            scale,
        }));
    }
}

/// The host's copy (`id`) of an own bolt takes over from it, if it is one:
/// the offset between them now (own time, the view's); `None` when the own
/// bolt has stopped (the copy is not drawn).
fn take_over(
    drawn: &mut Drawn,
    id: SimId,
    missile: &Missile,
    (own_now, now): (f64, f32),
) -> Option<Vec3> {
    let Some(own) = drawn.own_bolts.iter_mut().find(|own| {
        !matches!(own.fate, Fate::Handed(_)) && own.is(missile, own_now - f64::from(now))
    }) else {
        return Some(Vec3::ZERO);
    };
    if matches!(own.fate, Fate::Ended(_)) {
        return None;
    }
    let host = Vec3::from_array(missile.base)
        + Vec3::from_array(missile.velocity) * ((now - missile.from_ms as f32) / 1000.0);
    let handover = Handover {
        offset: own.at(own_now) - host,
        since: now,
    };
    // The copy leaves the gun as the own bolt did.
    if let Some(&launch) = drawn.launches.get(&(missile.owner, own.fired as u64)) {
        drawn
            .launches
            .insert((missile.owner, missile.from_ms), launch);
    }
    own.fate = Fate::Handed(id);
    drawn.handovers.insert(id, handover);
    drawn.own[1] += 1;
    Some(handover.offset)
}

/// The own bolts fly to own time: each stops at the first wall or body it
/// meets as drawn, a body's impact playing at once (foretelling the
/// authority's hit, as contact sparks do).
fn fly(
    drawn: &mut Drawn,
    (me, own_now, now): (SimId, f64, f32),
    (level, kotor): (&sim::Level, Option<&WeaponSounds>),
    current: &Snapshot,
    bodies: &[collision::Body],
    (cues, sparked): (&mut Vec<Cue>, &mut Vec<HookVisual>),
) {
    let doors = level.states_in(current);
    let mut met = Vec::new();
    for own in &mut drawn.own_bolts {
        let flying = !matches!(own.fate, Fate::Ended(_) | Fate::Handed(_));
        match own.fly(own_now, (level, doors.as_deref()), bodies, me) {
            Some((victim, point)) => met.push((own.weapon.base_item, victim, point, -own.velocity)),
            None if flying && matches!(own.fate, Fate::Ended(_)) => drawn.own[4] += 1,
            None => {}
        }
    }
    for (base_item, victim, point, back) in met {
        drawn.own[2] += 1;
        let at = point + back.normalize_or(Vec3::Z) * SURFACE;
        bolt_hit((cues, sparked), kotor, base_item, (at, now));
        drawn.shown.push((me, victim, now));
        drawn.contacts.shown += 1;
    }
}

/// KOTOR's blaster impact at `at` (units) at `ms`, and the bolt's impact
/// sound there (a weapon of `base_item`).
fn bolt_hit(
    (cues, sparked): (&mut Vec<Cue>, &mut Vec<HookVisual>),
    kotor: Option<&WeaponSounds>,
    base_item: u16,
    (at, ms): (Vec3, f32),
) {
    sparked.push(spark(BOLT_HIT, at));
    if let Some(sound) = kotor.and_then(|kotor| kotor.bolt_hit(base_item, false)) {
        cues.push(Cue::at(sound, at, ms));
    }
}

/// The impacts of the events new in `snapshot`: at the hit location's node
/// (as the bodies stand this frame) or the chest, out toward the attacker.
fn hits(
    drawn: &mut Drawn,
    snapshot: &Snapshot,
    (level, kotor): (&sim::Level, Option<&WeaponSounds>),
    hitboxes: &Hitboxes,
    places: &Query<&GlobalTransform>,
    (cues, sparked): (&mut Vec<Cue>, &mut Vec<HookVisual>),
) {
    let to_game = game_to_bevy().inverse();
    let objects = Instances::in_snapshot(level, snapshot);
    let catalog = level.scripts().map(|library| library.items());
    for entity in &snapshot.entities {
        let (Some(fighter), Some(player)) = (entity.fighter, entity.player) else {
            continue;
        };
        // A character first seen has seen its ring already.
        let seen = *drawn.seen.entry(entity.id).or_insert(fighter.next_event);
        let fresh = fighter.next_event.wrapping_sub(seen);
        drawn.seen.insert(entity.id, fighter.next_event);
        for event in fighter.events.iter().flatten() {
            if event.sequence.wrapping_sub(seen) >= fresh {
                continue;
            }
            if let (Outcome::Parried | Outcome::Deflected, Some(at)) = (event.result, event.at) {
                drawn.refuted(event.by, entity.id);
                let ms = snapshot.time_ms as f32;
                block(drawn, (event, at, ms), kotor, (cues, sparked));
                continue;
            }
            let struck = matches!(
                event.result,
                Outcome::Hit | Outcome::Critical | Outcome::Automatic
            );
            if !struck {
                continue;
            }
            let attacker = event.by.and_then(|by| cast::entity(snapshot, by));
            let struck_by = (event.weapon, attacker, entity);
            let bolt = match event.weapon {
                Some(Delivery::Saber(_) | Delivery::Melee) => {
                    if drawn.foretold(event.by, entity.id) {
                        continue;
                    }
                    false
                }
                Some(Delivery::Blaster(_)) => {
                    if drawn.foretold(event.by, entity.id) {
                        continue;
                    }
                    true
                }
                // Fists, claws and batons sound, with no effect.
                Some(Delivery::Unarmed | Delivery::Baton) => {
                    let at = Vec3::from_array(mind::chest(&player));
                    let mut rng = seed(entity.id.0, u32::from(event.sequence));
                    let met = (snapshot, &objects, catalog);
                    let ms = snapshot.time_ms as f32;
                    cues.extend(hit_sound(kotor, struck_by, met, (at, ms), &mut rng));
                    continue;
                }
                _ => continue,
            };
            let node = hitboxes
                .0
                .get(&entity.id)
                .and_then(|spots| spots[event.location as usize])
                .and_then(|node| places.get(node).ok())
                .map(|place| to_game * place.translation() / UNIT);
            let point = node.unwrap_or_else(|| Vec3::from_array(mind::chest(&player)));
            let toward = attacker
                .and_then(|a| a.player)
                .map(|a| Vec3::from_array(mind::chest(&a)) - point)
                .and_then(Vec3::try_normalize)
                .unwrap_or_else(|| Vec3::from_array(sim::math::angle_vectors(player.view).0));
            let mut rng = seed(entity.id.0, u32::from(event.sequence));
            let at = point + toward * SURFACE;
            let ms = snapshot.time_ms as f32;
            if bolt {
                let base_item = attacker.map_or(0, |by| cast::weapon(by, &objects).base_item);
                bolt_hit((cues, sparked), kotor, base_item, (at, ms));
                drawn.hits[0] += 1;
                continue;
            }
            let met = (snapshot, &objects, catalog);
            let round = round_hit(kotor, (attacker, entity), met, (at, ms), &mut rng);
            cues.extend(round.or_else(|| hit_sound(kotor, struck_by, met, (at, ms), &mut rng)));
            sparked.push(spark(HIT_SPARKS, at));
            drawn.hits[1] += 1;
        }
    }
    drawn
        .blocked
        .retain(|&(tick, _)| tick + 125 >= snapshot.tick);
    drawn.trim();
}

/// A block's effects at `at` (units): a blade's block, once a clash (both
/// its blades mark it), or a bolt's deflection, each facing whoever's blow
/// or shot it turned.
fn block(
    drawn: &mut Drawn,
    (event, at, ms): (&Event, [i32; 3], f32),
    kotor: Option<&WeaponSounds>,
    (cues, sparked): (&mut Vec<Cue>, &mut Vec<HookVisual>),
) {
    let point = Vec3::from_array(at.map(|v| v as f32));
    if event.result == Outcome::Deflected {
        sparked.push(spark(BOLT_DEFLECTED, point));
        if let Some(kotor) = kotor {
            cues.push(Cue::at(kotor.deflected(), point, ms));
        }
        drawn.blocks_seen[1] += 1;
        return;
    }
    if drawn.blocked.contains(&(event.tick, at)) {
        return;
    }
    drawn.blocked.push((event.tick, at));
    sparked.extend([PARRY_SPARKS, CLASH_SPARKS].map(|row| spark(row, point)));
    drawn.blocks_seen[0] += 1;
}

/// KOTOR's sparks of `row` at `at` (units).
fn spark(row: u16, at: Vec3) -> HookVisual {
    HookVisual {
        row,
        place: Place::At(at * UNIT),
    }
}

/// The contact sparks of the own blades (I22 C20) at `now`, each kept to
/// answer for the authority's hit it foretells.
fn sparks(
    drawn: &mut Drawn,
    contacts: Vec<Contact>,
    (now, met, kotor): (f32, Met, Option<&WeaponSounds>),
    (cues, sparked): (&mut Vec<Cue>, &mut Vec<HookVisual>),
) {
    for contact in contacts {
        let mut rng = seed(contact.victim.0, (now / FRAME_MS) as u32);
        sparked.push(spark(HIT_SPARKS, contact.point));
        let (current, objects, _) = met;
        let by = cast::entity(current, contact.by);
        if let Some(victim) = cast::entity(current, contact.victim) {
            let delivery = by.map(|by| cast::sheet(by, objects).main_weapon().delivery);
            let struck_by = (delivery, by, victim);
            cues.extend(hit_sound(
                kotor,
                struck_by,
                met,
                (contact.point, now),
                &mut rng,
            ));
        }
        drawn.shown.push((contact.by, contact.victim, now));
        drawn.contacts.shown += 1;
    }
    drawn.trim();
}

/// A scene's melee blow's hit sound (one of the two fighting a round: its
/// clip, `blows`, begun within the round): KOTOR's, its attacker's row in
/// hand (`WeaponSounds::in_hand`, a lightsaber's too) for what `struck`
/// wears, as KOTOR's clips sound every weapon; none for any other blow.
fn round_hit(
    kotor: Option<&WeaponSounds>,
    (by, struck): (Option<&EntityState>, &EntityState),
    met: Met,
    (at, ms): (Vec3, f32),
    rng: &mut Rng,
) -> Option<Cue> {
    use sim::script::effects::blows;
    let (snapshot, objects, _) = met;
    let (kotor, by) = (kotor?, by?);
    let in_round = |entity: &EntityState| {
        entity.fighter.and_then(|f| f.acting).is_some_and(|acting| {
            blows::of(acting.animation).is_some()
                && snapshot.time_ms < acting.since + sim::combat::ROUND_MS
        })
    };
    if !in_round(by) && !in_round(struck) {
        return None;
    }
    let name = kotor.hit(kotor.in_hand(by, objects), struck, met, rng)?;
    Some(Cue::at(name, at, ms))
}

/// A blow's hit sound on `struck` at `at` (`docs/COMBAT.md`, "Sounds"): a
/// lightsaber's one of JA's three; any other weapon's (fists and claws
/// too) its `weaponsounds.2da` row's for what the struck wears
/// (`weapon_sounds`), the row by what `by` holds (else by the blow's
/// delivery: a melee weapon's `Blade`, the rest `Unarmed`, a saber
/// wielder's kick too).
fn hit_sound(
    kotor: Option<&WeaponSounds>,
    (delivery, by, struck): (Option<Delivery>, Option<&EntityState>, &EntityState),
    met: Met,
    (at, ms): (Vec3, f32),
    rng: &mut Rng,
) -> Option<Cue> {
    let (snapshot, objects, _) = met;
    let row = match delivery {
        Some(Delivery::Saber(_)) => None,
        _ => {
            let held = by.and_then(|by| kotor?.row(by, snapshot, objects));
            let delivered = if delivery == Some(Delivery::Melee) {
                BLADE_ROW
            } else {
                UNARMED_ROW
            };
            Some(held.or_else(|| kotor?.labelled(delivered)).unwrap_or(0))
        }
    };
    let name = match (row, kotor) {
        (Some(row), Some(kotor)) => kotor.hit(row, struck, met, rng)?,
        (Some(_), None) => return None,
        (None, _) => {
            let pick = (rng.random() * BLADE_HIT_SOUNDS.len() as f32) as usize;
            BLADE_HIT_SOUNDS[pick].to_string()
        }
    };
    Some(Cue::at(name, at, ms))
}
