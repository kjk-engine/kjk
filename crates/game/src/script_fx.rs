//! The scripts' visual effects drawn (`EffectVisualEffect` applied to an
//! object or at a place, `EffectBeam`; `fighter::Visual`, an event
//! `Outcome::Visual` on the character it is on), by their row of
//! `visualeffects.2da` (found by its label, the effect's number):
//!
//! - A fire-and-forget row (`Type_FD` F) spawns its impact models where the
//!   event was put, else on the character, each playing its `impact` clip
//!   (else its first) once (`fx_model`: the frag blast's, whose emitters
//!   detonate), and sounds its `SoundImpact` there. The turret's loss
//!   (`k_pebo_hawkhit`: two `VFX_FNF_GRENADE_FRAGMENTATION` on the Ebon
//!   Hawk) is one; a minigame's thing hands its visuals to the party's
//!   leader at the thing's place (`script::engine`), so they show here
//!   while the party is hidden.
//! - An effect applied for a while or for good (the event's time; a
//!   permanent one while the character is there: `VFX_DUR_FORCE_WHIRLWIND`
//!   is an F row so applied) does the same as it begins, its models staying
//!   on the character (they follow it) for as long as it lasts: each
//!   plays its `impact` once, then its `duration` (else its own clip: the
//!   shields' `default`) round and round, then its `cessation` once as the
//!   effect ends. Its `SoundDuration` is held while
//!   it lasts, and its end models (`Ces_*_Node`) and `SoundCessastion` (the
//!   table's spelling) come as it ends.
//! - A beam (`EffectBeam`, a lasting row whose `ProgFX_Duration` the engine
//!   draws itself) draws the game's own beam model (`BEAMS`, as its
//!   executable picks them) from its source's casting hand as drawn (the left, JA's Force hand) to the character, the
//!   model's +Y toward it, its lightning and point-to-point emitters'
//!   reference nodes put on the character every frame (`model_fx`'s bolts
//!   and travelling particles). A source channelling lightning is drawn
//!   by `force_view` instead (JA's arcs).
//! - A glow (`ProgFX_Impact` or `ProgFX_Duration` 1401 to 1426: the energy
//!   shields, the Force powers' flashes, carbonite, stealth) puts the
//!   executable's texture for it (`GLOWS`) over the character's meshes as a
//!   shell (`glow_shell`), as it begins for `FLASH_MS` and while it lasts.
//! - One lasting on a character is also kept in the sim (its fighter's
//!   lasting visuals, by row): it ends as soon as a script removes it, and
//!   a character first seen (a new scene, a seat joining late) shows the
//!   ones it already carries, from then on (`end_removed`, `new_visuals`).
//! - The rest the executable draws itself (`Row::begins_with`,
//!   `Row::lasts_with`): a Force power's fizzle and resistance (1200 to
//!   1202, models at the hand, middle and head), Knight and Master Speed's
//!   widening of the player's view (1500, `OwnScreen`), Force Speed's blur
//!   of it (1601, 1602, `speed_blur`), the medal and Revan's masks on their
//!   hooks (1700 to 1702) and a distortion (1800, `glow_shell`). 2000 and
//!   2001 (the stealth pulse, invisibility) it has nothing for; 1300 (a
//!   MIRV's shot) is a projectile, not drawn here.
//! - A row that shakes the screen (`ShakeType` 2: `VFX_IMP_SCREEN_SHAKE`)
//!   shakes the player's camera, wherever the effect is put, from
//!   `ShakeDelay` seconds after it begins for `ShakeDuration` seconds
//!   (`Shake`), as the executable shakes it; a new shake takes over only if
//!   it lasts at least as long as the one under way has left. `ShakeType`
//!   1 (`VFX_IMP_OVERKILL`) is a pad's rumble, not drawn. Nothing else
//!   shakes the camera: no hit, damage or critical.
//!
//! A row's models stand by node: `Imp_Root_M_Node` at the character's feet
//! (a place's ground: the medium size, every character taken as one),
//! `Imp_Impact_Node` at its middle and `Imp_HeadCon_Node` at its head, a
//! power's fizzle at its hand. On a creature these are its model's
//! `impact`, `talkdummy` (its mouth: the executable hangs a row's head
//! models there, not on `headconjure`, a power's source, so a rancor's
//! `v_rancorexp_imp` bursts from its jaws) and `handconjure` nodes, and the
//! model stands where the node is drawn, following it (as drawn the frame
//! before, kept upright: its particles stay in the scene's space); on a
//! door or placeable its model's `<model>_impact`, `_head_hit` and
//! `_hand`. A model
//! without the node stands `IMPACT_M`, `HEAD_M` or `HAND_M` over the feet
//! (a door's or placeable's at its root, where the executable puts what it
//! finds no node for); a beam stands at its source's hand node's height.
//!
//! Each model is read off the frame once per scene, and a spawned copy
//! lives until it has ended, its clips are over and its particles are gone,
//! `LIFE_MS` past its end at most.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use asset_odyssey::hooks;
use asset_odyssey::texture::TextureIr;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use sim::combat::Outcome;
use sim::minigame::Anim;
use sim::module::ObjectKind;
use sim::{SimId, Snapshot};

use crate::cast::BodyRoots;
use crate::fx_model::{self, Drawing, Model, Spawned, Spawning};
use crate::glow_shell::{Glow, GlowMaterial, SceneMeshes};
use crate::heat_haze::HeatHazeMaterial;
use crate::lifecycle::Installer;
use crate::lifecycle_view::SceneCamera;
use crate::material::OdysseyMaterial;
use crate::scene::{ModelAssets, SceneContent, SceneModel, SceneRoot, Spawner};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::sound::{Added, Cue, Cues, Forget, Hold, Loops};

/// A character's middle, head and hand over its feet (metres) where its
/// model has no node for them [verify].
const IMPACT_M: f32 = 1.0;
const HEAD_M: f32 = crate::cast::HEAD_M;
const HAND_M: f32 = 1.2;
/// A character's box origin over its feet (metres): its position's height.
const ORIGIN_M: f32 = -sim::motion::SOLE_Z * collision::UNIT;
/// How long a spawned effect lives past its end at most (ms).
const LIFE_MS: f64 = 10_000.0;
/// The engine's own beams (`ProgFX_Duration` from `FIRST_BEAM` on), by
/// their models, as the game's executable picks them (read off the Mac
/// build): the lightning beams (small and large), the droids' disable and
/// destroy, the death field's tentacle, the drain, the flame spray, the
/// stun, cold and ion rays, Force Storm's arc, the droids' stun and the
/// shock.
const FIRST_BEAM: u16 = 608;
const BEAMS: [&str; 14] = [
    "v_lightns_dur",
    "v_lightnx_dur",
    "v_drddisab_dur",
    "v_drdkill_dur",
    "v_deathfld_dur",
    "v_drain_dur",
    "v_flame_dur",
    "v_stunray_dur",
    "v_coldray_dur",
    "v_ionray01_dur",
    "v_ionray02_dur",
    "v_fstorm_dur",
    "v_drdstun_dur",
    "v_fshock_dur",
];
/// The engine's glows (`ProgFX` from `FIRST_GLOW` on), by their textures,
/// as the executable lists them (`fx_tex_13` is not among them; the last is
/// stealth's): the energy shields', the Force powers' flashes, carbonite's
/// (`glow_shell`).
const FIRST_GLOW: u16 = 1401;
const GLOWS: [&str; 26] = [
    "fx_tex_01",
    "fx_tex_02",
    "fx_tex_03",
    "fx_tex_04",
    "fx_tex_05",
    "fx_tex_06",
    "fx_tex_07",
    "fx_tex_08",
    "fx_tex_09",
    "fx_tex_10",
    "fx_tex_11",
    "fx_tex_12",
    "fx_tex_14",
    "fx_tex_15",
    "fx_tex_16",
    "fx_tex_17",
    "fx_tex_18",
    "fx_tex_19",
    "fx_tex_20",
    "fx_tex_21",
    "fx_tex_22",
    "fx_tex_23",
    "fx_tex_24",
    "fx_tex_25",
    "fx_tex_26",
    "fx_tex_stealth",
];
/// How long a glow an effect begins with (`ProgFX_Impact`) lasts (ms): the
/// executable's length of every effect's beginning.
const FLASH_MS: u64 = 1000;
/// The models the executable hangs on a character's node while an effect
/// lasts (`ProgFX` from `FIRST_HOOKED` on): the medal, Revan's masks.
const FIRST_HOOKED: u16 = 1700;
const HOOKED: [(&str, &str); 3] = [
    ("v_medal_dur", "medalhook"),
    ("v_revmask1_dur", "revmask1hook"),
    ("v_revmask2_dur", "revmask2hook"),
];
/// Knight and Master Speed's widening of the player's view (`ProgFX` 1500):
/// its camera's field of view eased (half a cosine) to `WIDE_DEGREES` over
/// `WIDENING_MS`, held, and put back at once `WIDE_MS` after it began.
const WIDE_DEGREES: f32 = 135.0;
const WIDENING_MS: f64 = 750.0;
const WIDE_MS: u64 = 4750;
/// A screen shake's swing at its height (degrees), and the swing's period
/// (ms), as the executable shakes the camera (`Shake::angles`).
const SHAKE_DEGREES: f32 = 20.0;
const SHAKE_PERIOD_MS: f32 = 30.0;
/// `visualeffects.2da`'s `ShakeType` of a camera's shake.
const CAMERA_SHAKE: i32 = 2;

/// The frames a model is kept hidden after it is spawned: its entities
/// stand only after the first, and its keyed meshes get their own
/// materials (`Drawing::effects`) in the second. Shown before, the frag
/// blast's meshes its clip keeps clear at first (its shockwave) flash
/// white over the screen.
const HIDDEN_FRAMES: u32 = 2;

pub struct ScriptFxPlugin;

impl Plugin for ScriptFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OwnScreen>()
            .init_resource::<HookVisuals>()
            .add_systems(
                PostUpdate,
                (spawn, draw, glow)
                    .chain()
                    .run_if(resource_exists::<ScriptFx>)
                    .run_if(resource_exists::<PresentedSnapshot>)
                    .after(crate::play_view::Drawn)
                    .before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                forget_hooked
                    .run_if(not(resource_exists::<ScriptFx>))
                    .after(crate::play_view::Drawn),
            );
    }
}

/// With no script visuals to draw them, the clips' hooked visuals asked
/// for are dropped, not kept.
fn forget_hooked(mut hooked: ResMut<HookVisuals>) {
    if !hooked.0.is_empty() {
        hooked.0.clear();
    }
}

/// Where a row's model stands on its character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Node {
    Root,
    Impact,
    Head,
    Hand,
    /// A node of the character's model by name, the model hung from it.
    Hook(&'static str),
}

impl Node {
    /// The node of a model named `model` it stands on: a creature's
    /// `impact`, `talkdummy` and `handconjure`; a door's or placeable's
    /// (`rooted`) `<model>_impact`, `_head_hit` and `_hand`; a hook by its
    /// name; none for the feet.
    fn name(self, model: &str, rooted: bool) -> Option<String> {
        let (creature, suffix) = match self {
            Node::Root => return None,
            Node::Hook(name) => return Some(name.to_string()),
            Node::Impact => (hooks::IMPACT, "impact"),
            Node::Head => (hooks::TALK_DUMMY, "head_hit"),
            Node::Hand => (hooks::HAND_CONJURE, "hand"),
        };
        Some(if rooted {
            format!("{model}_{suffix}")
        } else {
            creature.to_string()
        })
    }

    /// Over the character's feet (metres), where its model has no node.
    fn height(self) -> f32 {
        match self {
            Node::Root => 0.0,
            Node::Impact => IMPACT_M,
            Node::Head => HEAD_M,
            Node::Hand => HAND_M,
            // Hung from its node, it stands there.
            Node::Hook(_) => 0.0,
        }
    }
}

/// A row: its models by node as it begins and as it ends, its beam's
/// model, its glows' textures as it begins (for `FLASH_MS`) and while it
/// lasts, and its sounds as it begins, while it lasts and as it ends.
#[derive(Debug, Clone, Default, PartialEq)]
struct Row {
    models: Vec<(Node, String)>,
    end_models: Vec<(Node, String)>,
    beam: Option<String>,
    flash: Option<String>,
    glow: Option<String>,
    /// The player's view widened as it begins, blurred while it lasts; the
    /// character a distortion while it lasts.
    widen: bool,
    blur: bool,
    distort: bool,
    /// Its camera shake: after how long, and for how long (ms).
    shake: Option<(u64, u64)>,
    sound: String,
    held: String,
    end_sound: String,
}

/// `names`' entry for `code` counted from `first`.
fn listed(code: u16, first: u16, names: &[&str]) -> Option<String> {
    names
        .get(usize::from(code.checked_sub(first)?))
        .map(|name| name.to_string())
}

impl Row {
    /// What the executable draws itself as it begins (`ProgFX_Impact`).
    fn begins_with(&mut self, code: u16) {
        match code {
            1200 => self.models.push((Node::Hand, "v_fizzle_imp".into())),
            1201 => self.models.push((Node::Impact, "v_fizzle_imp".into())),
            1202 => self.models.push((Node::Head, "v_fresist_imp".into())),
            1500 => self.widen = true,
            _ => self.flash = listed(code, FIRST_GLOW, &GLOWS),
        }
    }

    /// What the executable draws itself while it lasts (`ProgFX_Duration`).
    fn lasts_with(&mut self, code: u16) {
        match code {
            1601 | 1602 => self.blur = true,
            1800 => self.distort = true,
            _ => {
                self.beam = listed(code, FIRST_BEAM, &BEAMS);
                self.glow = listed(code, FIRST_GLOW, &GLOWS);
                let hooked = code
                    .checked_sub(FIRST_HOOKED)
                    .and_then(|at| HOOKED.get(usize::from(at)));
                if let Some(&(model, hook)) = hooked {
                    self.models.push((Node::Hook(hook), model.into()));
                }
            }
        }
    }

    /// What its end brings: its end models and sound, at once.
    fn end(&self) -> Option<Row> {
        (!self.end_models.is_empty() || !self.end_sound.is_empty()).then(|| Row {
            models: self.end_models.clone(),
            sound: self.end_sound.clone(),
            ..Row::default()
        })
    }

    /// Every model and sound it needs read.
    fn models(&self) -> impl Iterator<Item = &str> {
        self.models
            .iter()
            .map(|(_, name)| name.as_str())
            .chain(self.beam.as_deref())
    }

    fn textures(&self) -> impl Iterator<Item = &str> {
        self.flash.iter().chain(&self.glow).map(String::as_str)
    }

    fn sounds(&self) -> impl Iterator<Item = &str> {
        [&self.sound, &self.held]
            .into_iter()
            .map(String::as_str)
            .filter(|sound| !sound.is_empty())
    }
}

/// What a model's read gave: it and its textures, or nothing where the
/// install lacks it.
pub(crate) type Read = Option<(Arc<Model>, Arc<BTreeMap<String, TextureIr>>)>;

pub(crate) enum Loaded<T> {
    Loading(Task<T>),
    Ready(T),
}

impl<T> Loaded<T> {
    /// Ready once its task is done.
    pub(crate) fn poll(&mut self) -> Option<&T> {
        if let Loaded::Loading(task) = self
            && let Some(done) = block_on(poll_once(task))
        {
            *self = Loaded::Ready(done);
        }
        match self {
            Loaded::Ready(value) => Some(value),
            Loaded::Loading(_) => None,
        }
    }
}

/// Where an effect stands: on a character (following it), or at a place's
/// ground (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Anchor {
    On(SimId),
    At(Vec3),
}

/// The character and row a lasting effect is kept as in the sim (its
/// fighter's lasting visuals): it ends as soon as they no longer run
/// there (`end_removed`). None for a fleeting one, or one played by hand.
type Kept = Option<(SimId, u16)>;

/// An effect asked for: its row, where, from when to when (world ms; no
/// end for one that stays), a beam's source, and what keeps it.
struct Asked {
    row: Row,
    anchor: Anchor,
    since: u64,
    until: Option<u64>,
    source: Option<SimId>,
    kept: Kept,
}

/// A model of an effect in play: where it stands and on which node, from
/// when to when (world ms), a beam's source, and how many frames it has
/// been drawn.
struct Live {
    spawned: Spawned,
    anchor: Anchor,
    /// Hung from its hook of its character's model, it goes with it.
    hung: bool,
    /// The node of its character's model it stands at, as drawn.
    follows: Option<Entity>,
    /// Else it stands this high over its feet (its source's, a beam's), and
    /// a beam aims this high over its target's (metres).
    stands: f32,
    aims: f32,
    since: u64,
    until: Option<u64>,
    lasting: bool,
    source: Option<SimId>,
    frames: u32,
    kept: Kept,
}

/// What the executable does to the player's own screen, as of the frame
/// (`script_fx`'s): its view widened (from when to when, world ms), how
/// far it is blurred (the swoop's ratio; `speed_blur`) and its camera
/// shaken.
#[derive(Resource, Default)]
pub struct OwnScreen {
    pub widen: Option<(u64, u64)>,
    pub blur: f32,
    pub shake: Option<Shake>,
}

/// A camera shake: from when (world ms, its delay passed) and for how long
/// (ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shake {
    pub from: u64,
    pub total: u64,
}

impl Shake {
    /// Whether `next` takes over from it at `now` (world ms): it lasts at
    /// least as long as this one has left.
    fn yields_to(&self, next: &Shake, now: u64) -> bool {
        next.total >= (self.from + self.total).saturating_sub(now.max(self.from))
    }

    /// The camera's pitch and yaw turned at `now_ms` (degrees), `sides`
    /// each axis's roll (`true`: turned, else not), as the executable turns
    /// it: with `r` its ms left, `20 (1 + cos(2π r / total)) sin(2π r / 30)`
    /// degrees, each axis by that turned back or not at random.
    pub fn angles(&self, now_ms: f64, sides: [bool; 2]) -> [f32; 2] {
        let since = now_ms - self.from as f64;
        if since < 0.0 || since >= self.total as f64 {
            return [0.0; 2];
        }
        let left = (self.total as f64 - since) as f32;
        let tau = std::f32::consts::TAU;
        let swing = SHAKE_DEGREES
            * (1.0 + (tau * left / self.total as f32).cos())
            * (tau * left / SHAKE_PERIOD_MS).sin();
        sides.map(|turned| if turned { -swing } else { 0.0 })
    }
}

impl OwnScreen {
    /// The field of view (degrees) at `now_ms` for a view whose own is
    /// `normal`.
    pub fn fov(&self, normal: f32, now_ms: f64) -> f32 {
        match self.widen {
            Some((since, until)) if now_ms < until as f64 => {
                let t = ((now_ms - since as f64) / WIDENING_MS).clamp(0.0, 1.0);
                let eased = (1.0 - (std::f64::consts::PI * t).cos()) / 2.0;
                normal + (WIDE_DEGREES - normal) * eased as f32
            }
            _ => normal,
        }
    }
}

/// An effect on a character's player's screen, from when to when (world
/// ms).
struct LiveScreen {
    widen: bool,
    on: SimId,
    since: u64,
    until: u64,
    kept: Kept,
}

/// A glow on a character, from when to when (world ms).
struct LiveGlow {
    glow: Glow,
    on: SimId,
    since: u64,
    until: u64,
    kept: Kept,
}

/// A lasting effect's held sound: its name, where it stands, until when.
struct Held {
    name: String,
    anchor: Anchor,
    until: Option<u64>,
    kept: Kept,
}

/// An effect the engine puts somewhere and leaves there: its row, and where
/// (`Place`).
pub struct HookVisual {
    pub row: u16,
    pub place: Place,
}

/// Where a `HookVisual` stands: where a node of a character's model is
/// drawn, by name (a clip's `Contact` sparks at the saber hook,
/// `clip_events`), or at a point (game space, metres: where a blade met a
/// body or a blade, `combat_fx`).
pub enum Place {
    Hook { on: SimId, hook: &'static str },
    At(Vec3),
}

/// The `HookVisual`s asked for since the last frame.
#[derive(Resource, Default)]
pub struct HookVisuals(pub Vec<HookVisual>);

/// The effect `hook` asks for at `now` (world ms), standing at its point
/// or where its node was last drawn (game space): an impact's height over
/// the place under it (`Node::height`). None where the character has no
/// such node.
fn at_hook(
    hook: &HookVisual,
    table: &BTreeMap<u16, Row>,
    node: impl FnOnce(SimId, &str) -> Option<Vec3>,
    now: u64,
) -> Option<Asked> {
    let row = table.get(&hook.row)?;
    let point = match hook.place {
        Place::Hook { on, hook } => node(on, hook)?,
        Place::At(point) => point,
    };
    Some(Asked {
        row: row.clone(),
        anchor: Anchor::At(point - Vec3::Z * IMPACT_M),
        since: now,
        until: None,
        source: None,
        kept: None,
    })
}

/// The view's state, put in with a session (`install`).
#[derive(Resource)]
pub struct ScriptFx {
    installer: Option<Arc<Installer>>,
    /// The scene swap its models and table belong to.
    generation: u32,
    /// `visualeffects.2da`'s rows that draw or sound anything, by label.
    table: Option<Loaded<BTreeMap<u16, Row>>>,
    models: BTreeMap<String, Loaded<Read>>,
    /// The glows' textures read, and their images made, by name.
    textures: BTreeMap<String, Loaded<Option<TextureIr>>>,
    glow_images: BTreeMap<String, (Vec<Handle<Image>>, f32)>,
    /// The sounds brought into the bank, by resref.
    sounds: BTreeMap<String, Loaded<Option<audio::Clip>>>,
    /// Each character's next event unseen (its sequence number), and the
    /// visuals on objects seen (by their effect, `sim::visuals`).
    seen: BTreeMap<SimId, u16>,
    seen_on_objects: BTreeSet<u32>,
    /// Each character's hold as last seen: until when (`new_holds`).
    holds: BTreeMap<SimId, u64>,
    /// Effects waiting for their time, or their row's models or sounds.
    asked: Vec<Asked>,
    live: Vec<Live>,
    glows: Vec<LiveGlow>,
    screens: Vec<LiveScreen>,
    shake: Option<Shake>,
    held: Vec<Held>,
    /// The world time drawn last (ms).
    drawn_ms: Option<f64>,
}

/// A session's script effects, their models read from `installer`.
pub fn install(world: &mut World, installer: Option<Arc<Installer>>) {
    world.insert_resource(ScriptFx {
        installer,
        generation: 0,
        table: None,
        models: BTreeMap::new(),
        textures: BTreeMap::new(),
        glow_images: BTreeMap::new(),
        sounds: BTreeMap::new(),
        seen: BTreeMap::new(),
        holds: BTreeMap::new(),
        seen_on_objects: BTreeSet::new(),
        asked: Vec::new(),
        live: Vec::new(),
        glows: Vec::new(),
        screens: Vec::new(),
        shake: None,
        held: Vec::new(),
        drawn_ms: None,
    });
}

/// An effect sound's name in the bank.
fn sound_name(resref: &str) -> String {
    format!("vfx/{resref}")
}

/// `visualeffects.2da`'s rows that draw or sound anything, by their labels
/// (the effects' numbers).
fn table(installer: &Installer, module: &str) -> BTreeMap<u16, Row> {
    let read = || installer.resolver(module).ok()?.two_da("visualeffects");
    read().map_or_else(BTreeMap::new, |table| rows(&table))
}

/// `visualeffects.2da`'s (`table`) rows that draw or sound anything.
fn rows(table: &fmt_odyssey::twoda::TwoDa) -> BTreeMap<u16, Row> {
    let text = |row: usize, column: Option<usize>| {
        column
            .and_then(|column| table.get(row, column))
            .map(|cell| String::from_utf8_lossy(cell).trim().to_ascii_lowercase())
            .filter(|cell| !cell.is_empty() && cell != "****")
    };
    let column = |name: &str| table.column(name);
    let [begins, lasts] = ["progfx_impact", "progfx_duration"].map(column);
    let progfx = |row: usize, column: Option<usize>| text(row, column)?.parse::<u16>().ok();
    let [shakes, delay, duration] = ["shaketype", "shakedelay", "shakeduration"].map(column);
    let number = |row: usize, column: Option<usize>| text(row, column)?.parse::<f32>().ok();
    let shake = |row: usize| {
        let kind = number(row, shakes)?;
        let ms = |column| (number(row, column).unwrap_or(0.0).max(0.0) * 1000.0) as u64;
        (kind as i32 == CAMERA_SHAKE).then(|| (ms(delay), ms(duration)))
    };
    let sounds = ["soundimpact", "soundduration", "soundcessastion"].map(column);
    let nodes = |prefix: &str| {
        [
            ("root_m_node", Node::Root),
            ("impact_node", Node::Impact),
            ("headcon_node", Node::Head),
        ]
        .map(|(name, node)| (column(&format!("{prefix}_{name}")), node))
    };
    let (starts, ends) = (nodes("imp"), nodes("ces"));
    let models = |row: usize, nodes: &[(Option<usize>, Node)]| {
        nodes
            .iter()
            .filter_map(|&(column, node)| Some((node, text(row, column)?)))
            .collect::<Vec<_>>()
    };
    (0..table.cells.len())
        .filter_map(|row| {
            let [sound, held, end_sound] =
                sounds.map(|column| text(row, column).unwrap_or_else(String::new));
            let mut found = Row {
                models: models(row, &starts),
                end_models: models(row, &ends),
                shake: shake(row),
                sound,
                held,
                end_sound,
                ..Row::default()
            };
            if let Some(code) = progfx(row, begins) {
                found.begins_with(code);
            }
            if let Some(code) = progfx(row, lasts) {
                found.lasts_with(code);
            }
            // An effect's number is its row's label (1001 on), not its place.
            let id = String::from_utf8_lossy(table.row_labels.get(row)?)
                .trim()
                .parse::<u16>()
                .ok()?;
            (found != Row::default()).then_some((id, found))
        })
        .collect()
}

/// The screen shake's probes (`kjk sim probes --only "screen shake"`):
/// K1's table shakes the camera for `VFX_IMP_SCREEN_SHAKE` alone, and the
/// shake swings as the executable's does.
#[cfg(feature = "probes")]
pub fn shake_probes() -> sim::probe::Checks {
    use sim::probe::check;
    let shaking: Vec<(u16, (u64, u64))> = crate::k1_resolver()
        .and_then(|resolver| resolver.two_da("visualeffects"))
        .map_or_else(BTreeMap::new, |table| rows(&table))
        .into_iter()
        .filter_map(|(id, row)| Some((id, row.shake?)))
        .collect();
    let shake = Shake {
        from: 1000,
        total: 1000,
    };
    // 997.5 ms left: a swing's quarter, the sine's top, at the envelope's
    // height; 507.5 ms left: near its trough.
    let peak = shake.angles(1002.5, [true, false]);
    let middle = shake.angles(1492.5, [true, true]);
    let waiting = shake.angles(999.0, [true, true]);
    let after = shake.angles(2000.0, [true, true]);
    let longer = Shake {
        from: 1500,
        total: 400,
    };
    let shorter = Shake {
        from: 1500,
        total: 300,
    };
    vec![
        (
            "K1's screen shake row",
            check(shaking == [(6002, (0, 1000))], format!("{shaking:?}")),
        ),
        (
            "a shake's swing, its envelope and its time",
            check(
                (peak[0] + 40.0).abs() < 0.01
                    && peak[1] == 0.0
                    && middle[0].abs() < 0.1
                    && waiting == [0.0; 2]
                    && after == [0.0; 2],
                format!("peak {peak:?}, middle {middle:?}, before {waiting:?}, after {after:?}"),
            ),
        ),
        (
            "a new shake takes over if it lasts as long as what is left",
            check(
                shake.yields_to(&longer, 1600) && !shake.yields_to(&shorter, 1600),
                "400 ms over 400 left: yes; 300: no".into(),
            ),
        ),
    ]
}

/// The head node's probes (`kjk sim probes --only "effect head node"`): a
/// row's `Imp_HeadCon_Node` model hangs from a creature's `talkdummy`, as
/// the executable hangs it, so the Taris rancor's death
/// (`VFX_PLOT_TAR_RANCOR_DEATH`, 1045) bursts from its mouth: its model
/// (`c_rancor`) has that node and no `headconjure`.
#[cfg(feature = "probes")]
pub fn head_node_probes() -> sim::probe::Checks {
    use sim::probe::check;
    let resolver = crate::k1_resolver();
    let death = (resolver.as_ref())
        .and_then(|resolver| resolver.two_da("visualeffects"))
        .and_then(|table| rows(&table).remove(&1045))
        .map(|row| row.models);
    let node = Node::Head.name("c_rancor", false);
    let rancor = (resolver.as_ref())
        .and_then(|resolver| crate::viewer::model(resolver, "c_rancor").ok().flatten());
    let has = |name: &str| rancor.as_ref().and_then(|ir| ir.node_index(name)).is_some();
    let held = node.as_deref().is_some_and(has);
    vec![(
        "the rancor's death bursts from its mouth",
        check(
            death == Some(vec![(Node::Head, "v_rancorexp_imp".to_string())])
                && held
                && !has(hooks::HEAD_CONJURE),
            format!(
                "row 1045's models {death:?} on {node:?}: c_rancor has it {held}, headconjure {}",
                has(hooks::HEAD_CONJURE)
            ),
        ),
    )]
}

/// The glow texture effect `effect` (its `visualeffects.2da` label) puts
/// over its character while it lasts: its `ProgFX_Duration`'s (`GLOWS`).
pub(crate) fn lasting_glow(resolver: &vfs::resolver::Resolver, effect: u16) -> Option<String> {
    let table = resolver.two_da("visualeffects")?;
    let row = table
        .row_labels
        .iter()
        .position(|label| String::from_utf8_lossy(label).trim().parse() == Ok(effect))?;
    let cell = table.get(row, table.column("progfx_duration")?)?;
    let code = String::from_utf8_lossy(cell).trim().parse::<u16>().ok()?;
    listed(code, FIRST_GLOW, &GLOWS)
}

/// Model `name` and its textures, read for `module`.
pub(crate) fn read(installer: &Installer, module: &str, name: &str) -> Read {
    let resolver = installer.resolver(module).ok()?;
    let model = fx_model::load(&resolver, name, false)?;
    let textures = fx_model::textures(&resolver, std::iter::once(&model));
    Some((Arc::new(model), Arc::new(textures)))
}

/// Texture `name`, read for `module`.
fn read_texture(installer: &Installer, module: &str, name: &str) -> Option<TextureIr> {
    crate::viewer::texture(&installer.resolver(module).ok()?, name)
}

/// Sound `resref`, read for `module`.
fn read_sound(installer: &Installer, module: &str, resref: &str) -> Option<audio::Clip> {
    let resolver = installer.resolver(module).ok()?;
    let bytes = crate::sound::kotor_file(&resolver, resref)?;
    audio::Clip::kotor(&bytes).ok()
}

/// Each of `names` not asked for yet read in the background by `read`.
pub(crate) fn ask_for<T: Send + 'static>(
    asked: &mut BTreeMap<String, Loaded<T>>,
    names: Vec<String>,
    (installer, module): (&Arc<Installer>, &str),
    read: fn(&Installer, &str, &str) -> T,
) {
    for name in names {
        asked.entry(name.clone()).or_insert_with(|| {
            let (installer, module) = (installer.clone(), module.to_string());
            Loaded::Loading(
                AsyncComputeTaskPool::get().spawn(async move { read(&installer, &module, &name) }),
            )
        });
    }
}

/// Where a character's feet are (metres), if it is there: a door or
/// placeable stands on its own place.
pub(crate) fn feet(snapshot: &Snapshot, id: SimId) -> Option<Vec3> {
    snapshot.get(id).map(|entity| {
        let origin = if rooted(snapshot, id) { 0.0 } else { ORIGIN_M };
        Vec3::from_array(entity.position) - Vec3::Z * origin
    })
}

/// Whether `id` is a door or placeable: the executable looks for nodes no
/// placeable model has (`<name>_impact`, `_hand`, `_head`), so what it
/// carries, and its beams' ends, stand at its root [verify an impact's in
/// the game: the executable skips one it finds no node for].
fn rooted(snapshot: &Snapshot, id: SimId) -> bool {
    snapshot.get(id).is_some_and(
        |entity| matches!(entity.kind, sim::Kind::Object(kind, _) if kind != ObjectKind::Creature),
    )
}

/// Whether `source` channels lightning, whose beam `force_view` draws (JA's
/// arcs); a drain's is its script's (`v_drain_dur`).
fn channelling(snapshot: &Snapshot, source: SimId) -> bool {
    snapshot
        .get(source)
        .and_then(|entity| entity.fighter.as_ref())
        .and_then(|fighter| fighter.force.channel)
        .is_some_and(|channel| channel.power == sim::combat::force::Power::Lightning)
}

/// The visual events new on each character since the last frame: what
/// each asks for, from `table`. A character first seen has its past events
/// counted as seen.
fn new_visuals(
    snapshot: &Snapshot,
    table: &BTreeMap<u16, Row>,
    seen: &mut BTreeMap<SimId, u16>,
) -> Vec<Asked> {
    let mut asked = Vec::new();
    for entity in &snapshot.entities {
        let Some(fighter) = &entity.fighter else {
            continue;
        };
        let now = snapshot.time_ms;
        let Some(last) = seen.insert(entity.id, fighter.next_event) else {
            // First seen (a new scene, a seat joining late): what lasts on
            // it already, from now on, without its beginning.
            for (number, until, by) in fighter.effects.visuals(now) {
                let Some(row) = table.get(&number) else {
                    continue;
                };
                let source = Some(SimId(by)).filter(|by| row.beam.is_some() && *by != entity.id);
                if row.beam.is_some() && source.is_none_or(|by| channelling(snapshot, by)) {
                    continue;
                }
                asked.push(Asked {
                    row: Row {
                        flash: None,
                        widen: false,
                        sound: String::new(),
                        ..row.clone()
                    },
                    anchor: Anchor::On(entity.id),
                    since: now,
                    until: Some(until),
                    source,
                    kept: Some((entity.id, number)),
                });
            }
            continue;
        };
        // A count gone back (a prediction corrected) brings nothing new.
        let fresh = fighter.next_event.wrapping_sub(last);
        if fresh > u16::MAX / 2 {
            continue;
        }
        for event in fighter.newest().take(usize::from(fresh)) {
            let Some(visual) = event.visual.filter(|_| event.result == Outcome::Visual) else {
                continue;
            };
            let Some(row) = table.get(&visual.row) else {
                continue;
            };
            let source = event.by.filter(|_| visual.beam);
            if visual.beam
                && (row.beam.is_none() || source.is_none_or(|by| channelling(snapshot, by)))
            {
                continue;
            }
            let anchor = match event.at {
                Some(at) => Anchor::At(Vec3::from_array(at.map(|v| v as f32)) * collision::UNIT),
                None => Anchor::On(entity.id),
            };
            let until = (visual.ms > 0).then(|| now + u64::from(visual.ms));
            asked.push(Asked {
                row: row.clone(),
                anchor,
                since: now,
                until,
                source,
                kept: until.map(|_| (entity.id, visual.row)),
            });
        }
    }
    asked
}

/// The visual a hold on `fighter` shows while it lasts at `now`: its
/// state's (`Hold::state`, `state::visual`), as the games' executables add
/// one to the state as it takes hold.
fn hold_visual(fighter: &sim::combat::Fighter, now: u64) -> Option<u16> {
    let force = &fighter.force;
    (force.stunned_until > now)
        .then(|| sim::combat::effects::state::visual(force.hold.state()))
        .flatten()
}

/// The holds new on each character since the last frame (`seen`: each
/// one's hold's end as last seen): each its state's visual until the hold
/// ends, a stun's model and sound, nothing for a paralysis or a horror. A
/// character first seen held shows what lasts of it, without its beginning.
fn new_holds(
    snapshot: &Snapshot,
    table: &BTreeMap<u16, Row>,
    seen: &mut BTreeMap<SimId, u64>,
) -> Vec<Asked> {
    let now = snapshot.time_ms;
    let mut asked = Vec::new();
    for entity in &snapshot.entities {
        let Some(fighter) = &entity.fighter else {
            continue;
        };
        let until = fighter.force.stunned_until;
        let before = seen.insert(entity.id, until);
        if before == Some(until) {
            continue;
        }
        let Some((number, row)) =
            hold_visual(fighter, now).and_then(|number| Some((number, table.get(&number)?)))
        else {
            continue;
        };
        let row = match before {
            Some(_) => row.clone(),
            None => Row {
                flash: None,
                widen: false,
                sound: String::new(),
                ..row.clone()
            },
        };
        asked.push(Asked {
            row,
            anchor: Anchor::On(entity.id),
            since: now,
            until: Some(until),
            source: None,
            kept: Some((entity.id, number)),
        });
    }
    asked
}

/// The visuals new on doors and placeables since the last frame
/// (`Snapshot::visuals`): what each asks for, from `table`. One begun a
/// while before it was first seen (a new scene) shows what lasts of it,
/// without its beginning.
fn new_on_objects(
    snapshot: &Snapshot,
    table: &BTreeMap<u16, Row>,
    seen: &mut BTreeSet<u32>,
) -> Vec<Asked> {
    seen.retain(|id| snapshot.visuals.iter().any(|on| on.id == *id));
    let mut asked = Vec::new();
    for on in &snapshot.visuals {
        if !seen.insert(on.id) {
            continue;
        }
        let Some(row) = table.get(&on.visual.row) else {
            continue;
        };
        let source = Some(on.by).filter(|by| on.visual.beam && *by != on.on);
        if on.visual.beam
            && (row.beam.is_none() || source.is_none_or(|by| channelling(snapshot, by)))
        {
            continue;
        }
        let late = on.since + sim::visuals::SEEN_MS <= snapshot.time_ms;
        if late && on.until().is_some_and(|until| until <= snapshot.time_ms) {
            continue;
        }
        let row = match late {
            true => Row {
                flash: None,
                widen: false,
                sound: String::new(),
                ..row.clone()
            },
            false => row.clone(),
        };
        asked.push(Asked {
            row,
            anchor: Anchor::On(on.on),
            since: on.since,
            until: on.until(),
            source,
            kept: None,
        });
    }
    asked
}

/// Ends what the sim no longer keeps (a script removed it, or cleared its
/// character's effects, or its hold ended early): its models, glows,
/// screens and held sound now, its end models and sound now rather than
/// when it would have ended; one still waiting to begin never does.
fn end_removed(view: &mut ScriptFx, snapshot: &Snapshot) {
    let now = snapshot.time_ms;
    let running: BTreeSet<(SimId, u16)> = snapshot
        .entities
        .iter()
        .filter_map(|entity| Some((entity.id, entity.fighter.as_ref()?)))
        .flat_map(|(id, fighter)| {
            let held = hold_visual(fighter, now);
            fighter
                .effects
                .visuals(now)
                .map(|(row, ..)| row)
                .chain(held)
                .map(move |row| (id, row))
        })
        .collect();
    let gone = |kept: Kept| kept.is_some_and(|kept| !running.contains(&kept));
    let ended = |until: Option<u64>| Some(until.map_or(now, |until| until.min(now)));
    for live in view.live.iter_mut().filter(|live| gone(live.kept)) {
        live.until = ended(live.until);
    }
    for held in view.held.iter_mut().filter(|held| gone(held.kept)) {
        held.until = ended(held.until);
    }
    for glow in view.glows.iter_mut().filter(|glow| gone(glow.kept)) {
        glow.until = glow.until.min(now);
    }
    for screen in view.screens.iter_mut().filter(|screen| gone(screen.kept)) {
        screen.until = screen.until.min(now);
    }
    view.asked.retain_mut(|asked| {
        if !gone(asked.kept) {
            return true;
        }
        // An end comes now; a beginning not yet drawn, never.
        asked.since = asked.since.min(now);
        asked.until.is_none()
    });
}

/// The effect a console `vfx` asks for (`ROW TAG [SECONDS] [FROM]`): the
/// row on the creature or object tagged `TAG` (`player`: the first seat's body), for
/// that long (none: at once), a beam from the one tagged `FROM`.
fn asked_by_hand(
    snapshot: &Snapshot,
    level: &sim::Level,
    table: &BTreeMap<u16, Row>,
    words: &[String],
) -> Result<Asked, String> {
    let instances = sim::Instances::in_snapshot(level, snapshot);
    let tagged = |tag: &str| -> Result<SimId, String> {
        if tag.eq_ignore_ascii_case("player") {
            return snapshot
                .body(sim::Seat(0))
                .map(|body| body.id)
                .ok_or_else(|| "no player".to_string());
        }
        crate::cast::tagged(snapshot, &instances, tag)
            .ok_or_else(|| format!("nothing tagged {tag}"))
    };
    let number = words
        .first()
        .and_then(|word| word.parse::<u16>().ok())
        .ok_or("a row is a number")?;
    let row = table.get(&number).ok_or("no such row draws anything")?;
    let on = tagged(words.get(1).ok_or("a tag")?)?;
    let seconds = match words.get(2) {
        Some(word) => word.parse::<f32>().map_err(|_| "seconds")?,
        None => 0.0,
    };
    let source = words.get(3).map(|tag| tagged(tag)).transpose()?;
    let since = snapshot.time_ms;
    Ok(Asked {
        row: row.clone(),
        anchor: Anchor::On(on),
        since,
        until: (seconds > 0.0).then(|| since + (seconds * 1000.0) as u64),
        source,
        kept: None,
    })
}

/// Reads the table, notes the new visual events, reads their models and
/// sounds, and spawns each effect once its time has come and all of its
/// are read.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn spawn(
    mut view: ResMut<ScriptFx>,
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    scene: Res<SceneContent>,
    roots: Query<Entity, With<SceneRoot>>,
    mut assets: ModelAssets,
    (mut added, mut forget, mut hazes, mut cues): (
        ResMut<Added>,
        ResMut<Forget>,
        ResMut<Assets<HeatHazeMaterial>>,
        ResMut<Cues>,
    ),
    (mut glow_materials, hitboxes): (ResMut<Assets<GlowMaterial>>, Res<crate::cast::Hitboxes>),
    (bodies, children, placed, globals): (
        Option<Res<BodyRoots>>,
        Query<&Children>,
        Query<&SceneModel>,
        Query<&GlobalTransform>,
    ),
    (mut requested, mut hooked): (ResMut<console::VfxRequests>, ResMut<HookVisuals>),
    mut commands: Commands,
) {
    let view = &mut *view;
    let hooked = std::mem::take(&mut hooked.0);
    if swap.generation != view.generation {
        // Their models went with the old scene; their sounds leave the bank.
        view.generation = swap.generation;
        view.table = None;
        view.models.clear();
        view.seen.clear();
        view.holds.clear();
        view.seen_on_objects.clear();
        view.asked.clear();
        view.live.clear();
        for glow in view.glows.drain(..) {
            glow.glow.remove(&mut commands);
        }
        view.textures.clear();
        view.glow_images.clear();
        view.screens.clear();
        view.shake = None;
        view.held.clear();
        view.drawn_ms = None;
        forget
            .0
            .extend(view.sounds.keys().map(|resref| sound_name(resref)));
        view.sounds.clear();
    }
    end_removed(view, presented.0.current());
    let Some(installer) = view.installer.clone() else {
        return;
    };
    let module = swap.level.resources().to_string();
    let table = view.table.get_or_insert_with(|| {
        let (installer, module) = (installer.clone(), module.clone());
        Loaded::Loading(
            AsyncComputeTaskPool::get().spawn(async move { table(&installer, &module) }),
        )
    });
    let Some(table) = table.poll() else {
        return;
    };
    let current = presented.0.current();
    let mut fresh = new_visuals(current, table, &mut view.seen);
    fresh.extend(new_holds(current, table, &mut view.holds));
    fresh.extend(new_on_objects(current, table, &mut view.seen_on_objects));
    // The console's (`vfx ROW TAG [SECONDS] [FROM]`), as a script's.
    for words in std::mem::take(&mut requested.0) {
        match asked_by_hand(current, &swap.level, table, &words) {
            Ok(asked) => fresh.push(asked),
            Err(why) => warn!(target: "script_fx", "vfx {}: {why}", words.join(" ")),
        }
    }
    // The engine's at a node of a character's model, as last drawn.
    let node = |on: SimId, hook: &str| {
        let body = bodies.as_ref()?.0.get(&on).copied()?;
        let named = |_: &asset_odyssey::model::ModelIr| Some(hook.to_string());
        let (entity, _) = body_node(body, named, (&children, &placed), &scene)?;
        let drawn = globals.get(entity).ok()?;
        Some(crate::scene::game_to_bevy().inverse() * drawn.translation())
    };
    fresh.extend(
        hooked
            .iter()
            .filter_map(|hook| at_hook(hook, table, node, current.time_ms)),
    );
    // A lasting effect's end, asked for as it begins.
    for asked in &fresh {
        if let (Some(end), Some(until)) = (asked.row.end(), asked.until) {
            view.asked.push(Asked {
                row: end,
                anchor: asked.anchor,
                since: until,
                until: None,
                source: None,
                kept: asked.kept,
            });
        }
    }
    view.asked.extend(fresh);
    // What the effects asked for, read in the background.
    let wanted = |names: fn(&Row) -> Vec<&str>| -> Vec<String> {
        view.asked
            .iter()
            .flat_map(|asked| names(&asked.row).into_iter().map(str::to_string))
            .collect()
    };
    let (models, textures, sounds) = (
        wanted(|row| row.models().collect()),
        wanted(|row| row.textures().collect()),
        wanted(|row| row.sounds().collect()),
    );
    let reading = (&installer, module.as_str());
    ask_for(&mut view.models, models, reading, read);
    ask_for(&mut view.textures, textures, reading, read_texture);
    ask_for(&mut view.sounds, sounds, reading, read_sound);
    for (resref, sound) in &mut view.sounds {
        if let Loaded::Loading(_) = sound
            && let Some(Some(clip)) = sound.poll()
        {
            added.0.push((sound_name(resref), clip.clone()));
        }
    }
    for model in view.models.values_mut() {
        model.poll();
    }
    for texture in view.textures.values_mut() {
        texture.poll();
    }
    let Some(root) = roots.iter().next() else {
        return;
    };
    let now = current.time_ms;
    let ready = |asked: &Asked| {
        let models = asked
            .row
            .models()
            .all(|name| matches!(view.models.get(name), Some(Loaded::Ready(_))));
        let textures = asked
            .row
            .textures()
            .all(|name| matches!(view.textures.get(name), Some(Loaded::Ready(_))));
        let sounds = asked
            .row
            .sounds()
            .all(|name| matches!(view.sounds.get(name), Some(Loaded::Ready(_))));
        asked.since <= now && models && textures && sounds
    };
    let (due, waiting): (Vec<Asked>, Vec<Asked>) =
        std::mem::take(&mut view.asked).into_iter().partition(ready);
    view.asked = waiting;
    let heard = |sounds: &BTreeMap<String, Loaded<Option<audio::Clip>>>, name: &str| {
        matches!(sounds.get(name), Some(Loaded::Ready(Some(_))))
    };
    for asked in due {
        let Some(base) = (match asked.anchor {
            Anchor::At(at) => Some(at),
            Anchor::On(id) => feet(current, id),
        }) else {
            continue;
        };
        if heard(&view.sounds, &asked.row.sound) {
            cues.0.push(Cue::at(
                sound_name(&asked.row.sound),
                (base + Vec3::Z * IMPACT_M) / collision::UNIT,
                asked.since as f32,
            ));
        }
        if heard(&view.sounds, &asked.row.held) {
            view.held.push(Held {
                name: sound_name(&asked.row.held),
                anchor: asked.anchor,
                until: asked.until,
                kept: asked.kept,
            });
        }
        // The player's camera shaken, wherever it was put.
        if let Some((delay, total)) = asked.row.shake {
            let next = Shake {
                from: asked.since + delay,
                total,
            };
            if view
                .shake
                .is_none_or(|shake| shake.yields_to(&next, asked.since))
            {
                view.shake = Some(next);
            }
        }
        // Its glows over its character, as it begins and while it lasts; a
        // distortion while it lasts; its player's screen widened as it
        // begins, blurred while it lasts.
        if let Anchor::On(on) = asked.anchor {
            if let (true, Some(until)) = (asked.row.distort, asked.until) {
                view.glows.push(LiveGlow {
                    glow: Glow::distortion(&mut glow_materials),
                    on,
                    since: asked.since,
                    until,
                    kept: asked.kept,
                });
            }
            let screens = [
                (asked.row.widen, true, Some(asked.since + WIDE_MS), None),
                (asked.row.blur, false, asked.until, asked.kept),
            ];
            for (asked_for, widen, until, kept) in screens {
                if let (true, Some(until)) = (asked_for, until) {
                    view.screens.push(LiveScreen {
                        widen,
                        on,
                        since: asked.since,
                        until,
                        kept,
                    });
                }
            }
            let glows = [
                (&asked.row.flash, Some(asked.since + FLASH_MS), None),
                (&asked.row.glow, asked.until, asked.kept),
            ];
            for (name, until, kept) in glows {
                if let (Some(until), Some(name)) = (until, name)
                    && let Some(Loaded::Ready(Some(texture))) = view.textures.get(name)
                {
                    let frames = view
                        .glow_images
                        .entry(name.clone())
                        .or_insert_with(|| Glow::images(texture, assets.split().0))
                        .clone();
                    view.glows.push(LiveGlow {
                        glow: Glow::new(texture, frames, &mut glow_materials),
                        on,
                        since: asked.since,
                        until,
                        kept,
                    });
                }
            }
        }
        let beam = asked.row.beam.as_ref().map(|name| (Node::Hand, name));
        let models = asked
            .row
            .models
            .iter()
            .map(|(node, name)| (*node, name))
            .chain(beam);
        for (node, name) in models {
            let Some(Loaded::Ready(Some((model, textures)))) = view.models.get(name) else {
                continue;
            };
            // One on a node of its character's model hangs from it; one hung
            // from a hook needs it (the medal's is on the ceremony's
            // clothes, the masks' on Revan's models); a beam stands at its
            // source's hand.
            // A character's head, middle and casting hand (the left, JA's
            // Force hand) as drawn: a body JA's rig poses leaves its model's
            // own nodes for them where the model rests them.
            let posed = |id: SimId, node: Node| {
                let location = match node {
                    Node::Head => sim::combat::Location::Head,
                    Node::Impact => sim::combat::Location::Chest,
                    Node::Hand => sim::combat::Location::HandLeft,
                    Node::Root | Node::Hook(_) => return None,
                };
                hitboxes.0.get(&id)?[location as usize]
            };
            let on = |id: SimId, node: Node| {
                let rooted = rooted(current, id);
                let body = bodies.as_ref()?.0.get(&id).copied()?;
                let own = body_node(
                    body,
                    |ir| node.name(&ir.name, rooted),
                    (&children, &placed),
                    &scene,
                );
                match posed(id, node) {
                    Some(drawn) => Some((drawn, own.map_or(node.height(), |(_, height)| height))),
                    None => own,
                }
            };
            let beam = asked.source.filter(|_| node == Node::Hand);
            // A beam leaves its source's casting hand as drawn (as
            // `force_view`'s lightning).
            let at_node = match (asked.anchor, beam) {
                (Anchor::On(id), None) => on(id, node),
                (_, Some(source)) => posed(source, Node::Hand).map(|hand| (hand, 0.0)),
                _ => None,
            };
            let hooked = at_node.filter(|_| matches!(node, Node::Hook(_)));
            if hooked.is_none() && matches!(node, Node::Hook(_)) {
                continue;
            }
            let height = |id: Option<SimId>, node: Node| match id {
                Some(id) if rooted(current, id) => 0.0,
                Some(id) => on(id, node).map_or(node.height(), |(_, height)| height),
                None => node.height(),
            };
            let target = match asked.anchor {
                Anchor::On(id) => Some(id),
                Anchor::At(_) => None,
            };
            let stands = height(beam.or(target), node);
            let aims = height(target, Node::Impact);
            let (images, stores) = assets.split();
            let mut spawning = Spawning {
                spawner: Spawner::new(textures, scene.0.area_light(), images),
                stores,
                hazes: &mut hazes,
                textures,
                area: scene.0.area_light(),
                lit: scene.0.lit,
                root: hooked.map_or(root, |(entity, _)| entity),
                seed: u32::from(asked.since as u16) | 1,
                commands: &mut commands,
            };
            let spawned = spawning.spawn(model.copy(), true);
            view.live.push(Live {
                spawned,
                anchor: asked.anchor,
                hung: hooked.is_some(),
                follows: at_node
                    .filter(|_| hooked.is_none())
                    .map(|(entity, _)| entity),
                stands,
                aims,
                since: asked.since,
                until: asked.until,
                lasting: asked.until.is_some(),
                source: if node == Node::Hand {
                    asked.source
                } else {
                    None
                },
                frames: 0,
                kept: asked.kept,
            });
        }
    }
}

/// The clips an effect's model plays at `now_ms`: its `impact` (else, one
/// that is no lasting effect's, its first clip) once from its beginning;
/// a lasting one's `duration` round and round after it until its end, then
/// its `cessation` once.
fn phases(live: &Live, now_ms: f64) -> Vec<Anim> {
    let clips = &live.spawned.model.clips;
    let once = |name: &str, since: u64| Anim {
        name: name.to_string(),
        looping: false,
        overlay: false,
        since,
    };
    let impact = clips
        .get("impact")
        .map(|clip| (clip.length * 1000.0) as u64);
    if !live.lasting {
        return match impact {
            Some(_) => vec![once("impact", live.since)],
            None => live
                .spawned
                .model
                .placed
                .ir
                .animations
                .first()
                .map(|clip| vec![once(&clip.name, live.since)])
                .unwrap_or_else(Vec::new),
        };
    }
    let started = live.since + impact.unwrap_or(0);
    let ended = live.until.is_some_and(|until| now_ms >= until as f64);
    // Its clip while it lasts: `duration`, else its own (the shields'
    // `default`).
    let lasting = clips
        .contains_key("duration")
        .then_some("duration")
        .or_else(|| {
            live.spawned
                .model
                .placed
                .ir
                .animations
                .iter()
                .map(|clip| clip.name.as_str())
                .find(|name| !matches!(*name, "impact" | "cessation"))
        });
    match (now_ms < started as f64, ended, lasting) {
        (true, ..) => vec![once("impact", live.since)],
        (false, false, Some(name)) => vec![Anim {
            looping: true,
            ..once(name, started)
        }],
        (false, true, _) if clips.contains_key("cessation") => {
            vec![once("cessation", live.until.unwrap_or(started))]
        }
        _ => Vec::new(),
    }
}

/// Poses each effect where its character is and by its clips, puts a
/// beam's ends on its target, holds the lasting ones' sounds, and draws
/// their emitters; one done (ended, its clips over, its particles gone, or
/// `LIFE_MS` past its end) goes.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut view: ResMut<ScriptFx>,
    presented: Res<PresentedSnapshot>,
    mut posing: ParamSet<(Query<&mut Transform>, Drawing)>,
    (cameras, globals): (Query<Entity, SceneCamera>, Query<&GlobalTransform>),
    (mut facts, dump_due): (ResMut<console::DumpFacts>, Res<console::DumpDue>),
    (mut loops, mut carried): (ResMut<Loops>, ResMut<crate::scene::CarriedLights>),
    (swap, hitboxes): (Res<SceneSwap>, Res<crate::cast::Hitboxes>),
    mut commands: Commands,
) {
    let view = &mut *view;
    if dump_due.0 {
        let rows = match &view.table {
            Some(Loaded::Ready(rows)) => Some(rows.len()),
            _ => None,
        };
        facts.0.insert(
            "script_fx".into(),
            serde_json::json!({
                "rows": rows,
                "asked": view.asked.len(),
                "live": view.live.iter().map(|live| serde_json::json!({
                    "model": live.spawned.model.placed.ir.name,
                    "anims": live.spawned.anims.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
                    "since": live.since,
                    "until": live.until,
                    // On a node of its character (else a height over its feet).
                    "follows": live.follows.and_then(|node| globals.get(node).ok()).map(|node| {
                        (crate::scene::game_to_bevy().inverse() * node.translation()).to_array()
                    }),
                    "feet": match live.anchor {
                        Anchor::On(id) => feet(presented.0.current(), id).map(|at| at.to_array()),
                        Anchor::At(at) => Some(at.to_array()),
                    },
                    "stands": live.stands,
                    // Each emitter: node, its texture, blend and facing,
                    // reference node, its target, particles.
                    "sprays": live.spawned.sprays.iter().map(|spray| {
                        let emitter = &live.spawned.model.fx.emitters[spray.emitter];
                        (
                            emitter.node.clone(),
                            (&emitter.texture, format!("{:?} {:?}", emitter.blend, emitter.facing), emitter.grid),
                            spray.reference,
                            spray.target.map(|t| t.to_array()),
                            spray.particles(),
                        )
                    }).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "held": view.held.iter().map(|held| held.name.clone()).collect::<Vec<_>>(),
                // Each glow: on whom, until when, its shells.
                "glows": view.glows.iter().map(|live| (live.on.0, live.until, live.glow.shells()))
                    .collect::<Vec<_>>(),
                "textures": view.textures.keys().collect::<Vec<_>>(),
                "models": view.models.keys().collect::<Vec<_>>(),
            }),
        );
    }
    let (previous, current) = (presented.0.previous(), presented.0.current());
    let alpha = f64::from(presented.0.alpha());
    let now_ms =
        previous.time_ms as f64 + (current.time_ms as f64 - previous.time_ms as f64) * alpha;
    let base = |anchor: Anchor| match anchor {
        Anchor::At(at) => Some(at),
        Anchor::On(id) => feet(current, id),
    };
    // A lasting effect's sound, held while it lasts and its character is
    // there.
    view.held.retain(|held| {
        let at = base(held.anchor);
        let lasts = held.until.is_none_or(|until| now_ms < until as f64);
        if let (Some(at), true) = (at, lasts) {
            loops.0.push(Hold::at(
                held.name.clone(),
                (at + Vec3::Z * IMPACT_M) / collision::UNIT,
            ));
        }
        at.is_some() && lasts
    });
    if view.live.is_empty() {
        view.drawn_ms = None;
        carried.set_if_neq(crate::scene::CarriedLights(Vec::new()));
        return;
    }
    let dt = view
        .drawn_ms
        .replace(now_ms)
        .filter(|was| *was <= now_ms)
        .map_or(0.0, |was| ((now_ms - was) / 1000.0) as f32);
    // One whose character is gone ends there.
    for live in &mut view.live {
        if base(live.anchor).is_none() {
            let now = now_ms as u64;
            live.until = Some(live.until.map_or(now, |until| until.min(now)));
        }
    }
    {
        let mut transforms = posing.p0();
        for live in &mut view.live {
            // One hung from its character's node stays where it hangs.
            if let Some(at) = base(live.anchor).filter(|_| !live.hung)
                && let Ok(mut transform) = transforms.get_mut(live.spawned.root)
            {
                // A beam stands at its source's hand, its +Y toward the
                // target: every beam model's emitters fire, and its
                // reference nodes stand, along its +Y.
                let from = live.source.and_then(|source| feet(current, source));
                let drawn = live
                    .follows
                    .and_then(|node| globals.get(node).ok())
                    .map(|node| crate::scene::game_to_bevy().inverse() * node.translation());
                let stands = drawn.unwrap_or(from.unwrap_or(at) + Vec3::Z * live.stands);
                let target = at + Vec3::Z * live.aims;
                let way = (target - stands).normalize_or(Vec3::Y);
                let turn = match from {
                    Some(_) => Quat::from_rotation_arc(Vec3::Y, way),
                    None => Quat::IDENTITY,
                };
                *transform = Transform::from_translation(stands).with_rotation(turn);
            }
            // Its clips begin as it is first shown: a burst at its clip's
            // start (KOTOR's sparks detonate at 0) is not spent hidden.
            if live.frames < HIDDEN_FRAMES {
                live.since = live.since.max(now_ms.ceil() as u64);
            }
            live.spawned.anims = phases(live, now_ms);
            let anims = live.spawned.anims.clone();
            fx_model::animate(
                &mut live.spawned,
                &anims,
                (now_ms, dt),
                Vec::new(),
                &mut transforms,
            );
            if live.source.is_some()
                && let Some(target) = base(live.anchor)
            {
                aim_beam(&live.spawned, target + Vec3::Z * live.aims, &mut transforms);
            }
        }
    }
    // The lights of the lasting ones shown (a worn item's light): where
    // their nodes stand, for the scene's lights (`scene::CarriedLights`).
    let lit: Vec<crate::material::GpuLight> = {
        let transforms = posing.p0();
        view.live
            .iter()
            .filter(|live| live.lasting && live.frames >= HIDDEN_FRAMES)
            .filter_map(|live| Some((live, transforms.get(live.spawned.root).ok()?)))
            .flat_map(|(live, root)| {
                let ir = &live.spawned.model.placed.ir;
                ir.lights.iter().map(move |light| {
                    let local = crate::scene::to_mat4(&ir.nodes[light.node].world)
                        .transform_point3(Vec3::ZERO);
                    let position = crate::scene::game_to_bevy() * root.transform_point(local);
                    crate::material::GpuLight {
                        position_radius: position.extend(light.radius),
                        color_multiplier: Vec3::from_array(light.color).extend(light.multiplier),
                        kind: UVec4::new(u32::from(light.ambient_only), light.dynamic_type, 0, 0),
                    }
                })
            })
            .collect()
    };
    carried.set_if_neq(crate::scene::CarriedLights(lit));
    let mut drawing = posing.p1();
    let turn = cameras
        .iter()
        .next()
        .and_then(|camera| drawing.transforms.get(camera).ok())
        .map_or(Quat::IDENTITY, |t| t.rotation);
    let facing = fx_model::facing(turn);
    // What their bouncing particles meet: the level, the bodies as drawn.
    let current = presented.0.current();
    let states = swap.level.states_in(current);
    let bodies = crate::particle_surfaces::bodies(&hitboxes, &globals);
    let meets = |from: Vec3, to: Vec3| {
        crate::particle_surfaces::meets(&swap.level, states.as_deref(), &bodies, (from, to))
    };
    for live in &mut view.live {
        // Hidden until its materials are its own; shown while a clip plays
        // or a lasting one lasts; then its particles live out.
        let lasts = live.lasting && live.until.is_none_or(|until| now_ms < until as f64);
        let playing = lasts
            || !live.spawned.anims.is_empty() && {
                !fx_model::active(&live.spawned.model, &live.spawned.anims, now_ms).is_empty()
            };
        let ready = live.frames >= HIDDEN_FRAMES;
        live.frames += 1;
        if let Ok(mut shown) = drawing.visibility.get_mut(live.spawned.root) {
            *shown = if playing && ready {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        drawing.effects(&mut live.spawned, Some(&meets), (now_ms, dt), facing);
    }
    view.live.retain(|live| {
        let end = live.until.map_or(live.since, |until| until.max(live.since)) as f64;
        let ended = live.until.is_some() || !live.lasting;
        let playing =
            !fx_model::active(&live.spawned.model, &live.spawned.anims, now_ms).is_empty();
        let particles = live
            .spawned
            .sprays
            .iter()
            .any(|spray| spray.particles() > 0);
        let keep = !(ended && now_ms >= end) || (playing || particles) && now_ms < end + LIFE_MS;
        if !keep {
            live.spawned.despawn(&mut commands);
        }
        keep
    });
}

/// Covers each glowing character's meshes with its glow's shells and plays
/// its texture; a glow ended, or whose character is gone, takes them off.
#[allow(clippy::too_many_arguments)]
fn glow(
    mut view: ResMut<ScriptFx>,
    presented: Res<PresentedSnapshot>,
    roots: Option<Res<BodyRoots>>,
    (children, meshes): (Query<&Children>, SceneMeshes),
    (odyssey, mut materials): (Res<Assets<OdysseyMaterial>>, ResMut<Assets<GlowMaterial>>),
    (own, mut screen): (Option<Res<OwnSeat>>, ResMut<OwnScreen>),
    mut commands: Commands,
) {
    let current = presented.0.current();
    let now = current.time_ms;
    // The player's own screen: the newest widening on its body, a blur.
    let me = crate::cast::me(current, own.map_or(sim::Seat(0), |own| own.0));
    view.screens.retain(|live| now < live.until);
    let mine = || {
        view.screens
            .iter()
            .filter(|live| Some(live.on) == me && live.since <= now)
    };
    let widen = mine()
        .filter(|live| live.widen)
        .max_by_key(|live| live.since)
        .map(|live| (live.since, live.until));
    let blur = if mine().any(|live| !live.widen) {
        1.0
    } else {
        0.0
    };
    let shake = view.shake.filter(|shake| now < shake.from + shake.total);
    view.shake = shake;
    if screen.widen != widen || screen.blur != blur || screen.shake != shake {
        *screen = OwnScreen { widen, blur, shake };
    }
    view.glows.retain_mut(|live| {
        let root = roots
            .as_ref()
            .and_then(|roots| roots.0.get(&live.on).copied());
        let Some(root) = root.filter(|_| now < live.until) else {
            live.glow.remove(&mut commands);
            return false;
        };
        live.glow
            .cover(root, (&children, &meshes), &odyssey, &mut commands);
        let seconds = now.saturating_sub(live.since) as f32 / 1000.0;
        live.glow.play(seconds, &mut materials);
        true
    });
}

/// The entity of the node `name` picks of the model under `root` (its own,
/// or one hung from it: a head), if it has one, and its rest height over
/// that model's root (metres).
fn body_node(
    root: Entity,
    name: impl Fn(&asset_odyssey::model::ModelIr) -> Option<String>,
    (children, placed): (&Query<&Children>, &Query<&SceneModel>),
    scene: &SceneContent,
) -> Option<(Entity, f32)> {
    std::iter::once(root)
        .chain(children.iter_descendants(root))
        .find_map(|entity| {
            let model = placed.get(entity).ok()?;
            let ir = &scene.0.models.get(model.index)?.ir;
            let node = ir.node_index(&name(ir)?)?;
            Some((*model.nodes.get(node)?, ir.nodes[node].world[2][3]))
        })
}

/// A beam's ends on its target: every lightning and point-to-point
/// emitter's reference node of `spawned` moved to `target` (game space),
/// whatever its clips pose.
fn aim_beam(spawned: &Spawned, target: Vec3, transforms: &mut Query<&mut Transform>) {
    let ir = &spawned.model.placed.ir;
    let placed: Vec<(Entity, Vec3)> = spawned
        .sprays
        .iter()
        .filter_map(|spray| {
            let reference = spray.reference?;
            let local = |e: Entity| transforms.get(e).ok().map(|t| t.to_matrix());
            let root = local(spawned.root)?;
            let parent = ir.nodes[reference].parent.map_or(Mat4::IDENTITY, |parent| {
                fx_model::node_matrix(spawned, parent, local)
            });
            let at = (root * parent).inverse().transform_point3(target);
            Some((spawned.nodes[reference], at))
        })
        .collect();
    for (entity, at) in placed {
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation = at;
        }
    }
}
