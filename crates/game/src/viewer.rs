//! Loads what the viewer draws through the resolver: one model (`kjk view`)
//! or every room of a module at its layout position (`kjk module`), with the
//! textures their meshes name.

use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::creature;
use asset_odyssey::hooks;
use asset_odyssey::model::{Matrix, ModelIr};
use asset_odyssey::module::{AreaGrass, AreaWeather, Kind, Missing, ModuleIr, Source};
use asset_odyssey::texture::TextureIr;
use asset_odyssey::weather::Fall;
use bevy::log::warn;
use bevy::math::Vec3;
use collision_odyssey::Surfaces;
use fmt_odyssey::ResType;
use fmt_odyssey::bwm::Walkmesh;
use fmt_odyssey::mdl::Model;
use fmt_odyssey::texture::texture_txi;
use vfs::resolver::Resolver;
use vfs::{Discovery, Game};

use crate::material::{AreaLight, Fog};
use crate::model_fx::{Chunk, ModelFx};
use crate::weapons::{Arms, Owner};

/// What `Viewer::place_appearance` dresses a creature in: its
/// `appearance.2da` row, its body item and how it came to wear it, its
/// `heads.2da` row, and its headgear.
pub type Outfit<'a> = (
    u32,
    (Option<&'a str>, creature::Dressing),
    Option<u8>,
    Option<&'a str>,
);

/// A model placed in the scene at an offset, turned by `facing` radians
/// about Z (game space).
#[derive(Clone)]
pub struct Placed {
    pub ir: ModelIr,
    pub offset: [f32; 3],
    pub facing: f32,
    /// Rooms, and doors and placeables marked `Static`: reone lights them
    /// only with lights of dynamic type 1.
    pub is_static: bool,
    /// A transform inside the placement, applied first: a head sits at its
    /// body's `headhook` node (its rest transform).
    pub attach: Option<Matrix>,
    /// (index of the placed body, its node): the head follows that node as
    /// the body animates.
    pub hook: Option<(usize, usize)>,
    /// The model root's transform under the hook node (a saber hilt sits at
    /// its hand bolt); identity when `None`.
    pub hook_local: Option<Matrix>,
    /// The clip to loop, with the clips of the model's supermodel chain.
    pub clip: Option<Arc<AnimationIr>>,
    /// A creature's body and head: the env map its appearance asks for
    /// (`creature::EnvMap`). None for any other model.
    pub envmap: Option<WornEnvMap>,
}

/// The env map a creature's body and head ask for: the cube map they are
/// drawn with where their meshes' textures name none, none where the area
/// gives none. Their textures' alpha is that map's mask, never drawn
/// see-through (`material::worn_surface`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WornEnvMap(pub Option<String>);

impl WornEnvMap {
    /// The cube map's name, where there is one.
    pub fn name(this: Option<&WornEnvMap>) -> Option<&String> {
        this.and_then(|worn| worn.0.as_ref())
    }
}

impl Placed {
    /// `ir` at `offset` (game space), facing +X, lit as a moving thing.
    pub fn at(ir: ModelIr, offset: [f32; 3]) -> Placed {
        Placed {
            ir,
            offset,
            facing: 0.0,
            is_static: false,
            attach: None,
            hook: None,
            hook_local: None,
            clip: None,
            envmap: None,
        }
    }
}

/// A module room for culling: which model draws it, what it sees, and its
/// walkable floor (world space) to tell which room a point is in.
pub struct RoomView {
    pub name: String,
    /// Index into `LoadedScene::models`; `None` when the model is missing.
    pub model: Option<usize>,
    pub visible: Vec<String>,
    /// The room's area has no VIS file. KOTOR 2 then turns its room culling
    /// off and draws every room from every room, so every room sees this one
    /// and this one sees every room (`fight_rooms::linked`).
    pub sees_all: bool,
    /// Empty for a room that is only seen (sky, backdrops, cutscene
    /// sets): the game ships it no walkmesh, or one without a walkable
    /// face, so nothing stands in it (reone takes an object's room from the walkable face under it).
    pub floor: Vec<[[f32; 3]; 3]>,
    /// Why the room's walkmesh could not be read; its floor is then empty
    /// for our reading, not the game's.
    pub floor_error: Option<String>,
}

/// What a room's or placeable's own clips and emitters do (`own_fx`): the
/// clips it plays, and what they and its MDL key beyond places and turns
/// (its emitters, its meshes' alpha and self-illumination).
#[derive(Clone)]
pub struct OwnFx {
    /// Index into `LoadedScene::models`.
    pub model: usize,
    pub plays: Plays,
    pub fx: Arc<ModelFx>,
    pub chunks: Arc<BTreeMap<String, Chunk>>,
}

/// A placeable's own clips and emitters, if it has any, from its model's
/// MDL as parsed and its IR (read by `resolver`, supermodels through
/// `cache`): object `object` placed in look `placed` (`own_fx`); its model
/// is the caller's to set. Its clips are its model's own and the clips its
/// looks play (`own_fx::look_clips`) its supermodel chain has, as the
/// executable finds a placeable's animation by name through the chain: a
/// "Sitting" placeable is a creature's model with no clip of its own.
pub(crate) fn placeable_fx(
    (cache, resolver, game): (&mut BTreeMap<String, Option<ModelIr>>, &Resolver, Game),
    parsed: &Model,
    ir: &ModelIr,
    (object, placed): (u32, u8),
) -> Option<OwnFx> {
    let fx = fx_through_chain(resolver, parsed);
    let clips = crate::placeable_light::clips(cache, resolver, game, ir);
    (!clips.is_empty() || !fx.emitters.is_empty()).then(|| OwnFx {
        model: 0,
        plays: Plays::Look {
            object,
            placed,
            clips,
        },
        chunks: Arc::new(fx.chunks(resolver)),
        fx: Arc::new(fx),
    })
}

/// The clips a model of the scene plays of itself.
#[derive(Clone)]
pub enum Plays {
    /// A room's `animloop…` clips, all looping at once as reone's area
    /// plays them, in the model's order (on a node two of them key, the
    /// later one's keys are shown [verify: reone's order is a hash map's]),
    /// and the `scriptloopNN` clips a script may start on it
    /// (`PlayRoomAnimation`), keyed by NN; `room` is `sim::room_key` of its
    /// name.
    Room {
        loops: Vec<Arc<AnimationIr>>,
        scripted: BTreeMap<u8, Arc<AnimationIr>>,
        room: u32,
    },
    /// A placeable's clips by lowercase name, the one its look picks
    /// playing (`own_fx`): object `object` of the area's instances, placed
    /// in look `placed` (`sim::look`; the look where no sim runs).
    Look {
        object: u32,
        placed: u8,
        clips: BTreeMap<String, Arc<AnimationIr>>,
    },
    /// A creature's body: posed by the cast (`cast`), whose clips key its
    /// emitters and pass their `detonate` events (`own_fx::BodyClips`), as
    /// the executable draws a creature model's emitters (the welding
    /// droid's sparks, a broken war droid's).
    Body,
}

impl OwnFx {
    /// This placeable's, as model `model`, object `object` placed in look
    /// `placed`.
    pub fn placed_as(&self, model: usize, (object, placed): (u32, u8)) -> OwnFx {
        let plays = match &self.plays {
            Plays::Look { clips, .. } => Plays::Look {
                object,
                placed,
                clips: clips.clone(),
            },
            loops => loops.clone(),
        };
        OwnFx {
            model,
            plays,
            ..self.clone()
        }
    }
}

impl Plays {
    /// Every clip it may play.
    pub fn clips(&self) -> Box<dyn Iterator<Item = &Arc<AnimationIr>> + '_> {
        match self {
            Plays::Room {
                loops, scripted, ..
            } => Box::new(loops.iter().chain(scripted.values())),
            Plays::Look { clips, .. } => Box::new(clips.values()),
            Plays::Body => Box::new(std::iter::empty()),
        }
    }
}

#[derive(Default)]
pub struct LoadedScene {
    pub name: String,
    pub models: Vec<Placed>,
    pub textures: BTreeMap<String, TextureIr>,
    /// The images `textures` were uploaded as, which the characters and
    /// placeables spawned later take again (`scene::SharedImages`).
    pub images: crate::scene::SharedImages,
    /// Texture names the meshes use but the install does not resolve, or
    /// that do not decode (`stand_in` says what is drawn instead).
    pub missing: Vec<String>,
    /// Body textures `appearance.2da` names that the install lacks (also in
    /// `missing`): those bodies keep their model's own textures.
    pub missing_body_textures: Vec<String>,
    /// For a module: where the player enters, and facing (x, y).
    pub entry: Option<([f32; 3], [f32; 2])>,
    /// Room models the layout names but the install lacks.
    pub missing_models: Vec<String>,
    /// Empty for a lone model.
    pub rooms: Vec<RoomView>,
    /// The rooms with `animloop…` clips or emitters of their own, and the
    /// placeables with clips or emitters (`own_fx`).
    pub own_fx: Vec<OwnFx>,
    /// Doors and placeables drawn (the last entries of `models`).
    pub instances: usize,
    /// Creatures drawn, and the ones whose look did not resolve.
    pub creatures: usize,
    pub creature_refusals: Vec<String>,
    /// Creatures whose idle clip is in no model of their chain.
    pub missing_clips: Vec<String>,
    /// One line per creature: models and hook positions (for checking).
    pub creature_notes: Vec<String>,
    /// Weapon models placed in creatures' hands.
    pub weapons: usize,
    /// The area's ambient light for objects without lightmaps.
    pub ambient: [f32; 3],
    /// The area's distance fog, when it has one.
    pub fog: Option<Fog>,
    /// The area's placed cameras (its GIT's), the shots a conversation's
    /// line may name.
    pub cameras: Vec<asset_odyssey::module::Camera>,
    /// `dialoganimations.2da`'s `CU_PB_Range` by row (metres, 0 where
    /// blank): how far the dialog camera's close-up pulls back from a
    /// speaker playing that row's talk clip.
    pub pull_backs: Vec<f32>,
    /// How the area's picture shows it (the minimap's), when it says.
    pub map: Option<asset_odyssey::map::AreaMap>,
    /// The conversations' camera models (`CameraModel`, whose `camerahook`
    /// carries an animated line's camera) and their cutscenes' stunt models
    /// (`StuntList`), by lowercase name.
    pub cutscene_models: BTreeMap<String, Arc<ModelIr>>,
    /// Whether runtime lighting applies; a lone model (`kjk view`) has no
    /// area or lights and is drawn unlit.
    pub lit: bool,
    /// Saber blades, drawn each frame from their hilt.
    pub blades: Vec<BladeSpec>,
    /// Models shown only part of the time.
    pub shown: Vec<Shown>,
    /// Effects drawn around the camera each frame.
    pub effects: Effects,
    /// A module's doors, to open and close as the sim has them.
    pub doors: Vec<DoorView>,
    /// Each placed door and placeable's model index, by its object (its
    /// index in the area's instances): a cutscene moves the ones it names.
    pub instance_models: BTreeMap<u32, usize>,
    /// A module's creatures, to move and animate as the sim has them.
    pub creature_views: Vec<CreatureView>,
    /// In a play scene, every character's weapon models (`weapons::arm`).
    pub arms: BTreeMap<Owner, Arms>,
    /// The area's grass, when its ARE has some (`grass`).
    pub grass: Option<SceneGrass>,
    /// What the area's weather goes by (none outside a module).
    pub weather: SceneWeather,
    /// A module's sound, read with it for the world's first frame
    /// (`ambience`); `None` where nothing plays it.
    pub sound: Option<crate::ambience::Preread>,
    /// The area's `DefaultEnvMap`: the env map of a body whose appearance
    /// asks for the area's (`creature::EnvMap::AreaDefault`).
    pub default_envmap: Option<String>,
    /// The area's light probes (`light_probes`), made as it loads. Empty
    /// outside a module, and for an area with no lightmapped room mesh.
    pub probes: Arc<crate::light_probes::AreaProbes>,
    /// Whether the area is drawn under the baked trial
    /// (`fitted_lights::baked_area` as it loaded). Under the options' Enhanced
    /// Lightmaps this is only an area with a bake the game accepted.
    pub baked: bool,
    /// Whether a used bake of the area asks for its rooms' surfaces with no
    /// lightmap to be dark (`dark_unlit`).
    pub dark_unlit: bool,
    /// The lightmaps the game's data stores where the area shows a baked
    /// one in their place (`baked_lightmaps`), by name in lower case. The
    /// fitted lights are levelled against the stored light
    /// (`fitted_lights::level`), whichever lightmap the rooms show. Empty
    /// for an area that shows no bake.
    pub stored_lightmaps: BTreeMap<String, TextureIr>,
}

/// What an area's weather goes by (`weather`).
#[derive(Default)]
pub struct SceneWeather {
    /// The ARE's weather chances and wind.
    pub area: AreaWeather,
    /// The area's resref, which its weather rolls go by (`weather::draw`).
    pub resref: String,
    /// Whether each room (as `LoadedScene::rooms`) keeps the weather out
    /// (the ARE's `DisableWeather`).
    pub dry_rooms: Vec<bool>,
    /// The model the area's rain or snow falls from, when it has a chance
    /// of either.
    pub model: Option<Arc<WeatherModel>>,
    /// What the area's lightning plays, when it has a chance of it
    /// (`weather::Lightning`).
    pub lightning: Option<Arc<crate::weather::Lightning>>,
}

/// The model an area's rain or snow falls from, the fall it draws, and its
/// textures.
pub struct WeatherModel {
    pub fall: Fall,
    pub model: crate::fx_model::Model,
    pub textures: BTreeMap<String, TextureIr>,
}

/// The weather model of an area with `weather`: `fx_rain` while it has a
/// chance of rain (rain wins), else `fx_snow` while it has one of snow; none
/// for an area with neither (the executable makes no rain objects without a
/// rain chance: Malachor's lightning falls on nothing) or an install without
/// the model.
fn weather_model(resolver: &Resolver, weather: &AreaWeather) -> Option<WeatherModel> {
    let (fall, name) = match (weather.rain, weather.snow) {
        (0, 0) => return None,
        (0, _) => (Fall::Snow, "fx_snow"),
        _ => (Fall::Rain, "fx_rain"),
    };
    let model = crate::fx_model::load(resolver, name, false)?;
    let textures = crate::fx_model::textures(resolver, std::iter::once(&model));
    Some(WeatherModel {
        fall,
        model,
        textures,
    })
}

/// One room's grass: its tufts (world, game space) and the lightmap of the
/// walkmesh they stand on.
#[derive(Debug, Clone, PartialEq)]
pub struct GrassPatch {
    pub room: usize,
    pub tufts: Vec<asset_odyssey::grass::Tuft>,
    pub lightmap: Option<String>,
}

/// The area's grass as drawn: its texture and quad size, its colours, the
/// `WindPower` that sways it, and each room's patch. A texture the install
/// lacks leaves no patches (`missing_texture`).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneGrass {
    pub texture: String,
    pub quad_size: f32,
    pub ambient: Vec3,
    pub diffuse: Vec3,
    pub emissive: Vec3,
    pub wind: f32,
    pub patches: Vec<GrassPatch>,
    pub missing_texture: bool,
}

impl SceneGrass {
    /// `grass` without its patches yet; `missing_texture` when the install
    /// has neither a TPC nor a TGA of its name.
    fn of(resolver: &Resolver, grass: &AreaGrass, wind: i32) -> SceneGrass {
        let texture = grass.texture.to_ascii_lowercase();
        let found = [ResType::TPC, ResType::TGA]
            .into_iter()
            .any(|kind| resolver.find(&texture, kind).is_some());
        let colour = |packed: u32| Vec3::from_array(unpack_colour(packed));
        SceneGrass {
            texture,
            quad_size: grass.quad_size,
            ambient: colour(grass.ambient),
            diffuse: colour(grass.diffuse),
            emissive: colour(grass.emissive),
            wind: wind as f32,
            patches: Vec::new(),
            missing_texture: !found,
        }
    }

    /// The textures it draws with: its own and its patches' lightmaps.
    fn textures(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.texture).chain(
            self.patches
                .iter()
                .filter_map(|patch| patch.lightmap.as_ref()),
        )
    }
}

impl LoadedScene {
    pub fn area_light(&self) -> AreaLight {
        AreaLight {
            ambient: self.ambient,
            fog: self.fog,
            shadowed: self.lit,
            probed: self.lit,
            dark_unlit: self.baked && self.dark_unlit,
            fitted: crate::fitted_lights::wanted_in(self.baked),
        }
    }
}

/// A door drawn in the scene: which object of the module it is (its index
/// in the area's instances), its model, and its clips.
pub struct DoorView {
    pub object: u32,
    pub model: usize,
    /// `opening1`, `opening2`, `closing1`, `closing2`, from the model or its
    /// supermodels.
    pub opening: [Option<Arc<AnimationIr>>; 2],
    pub closing: [Option<Arc<AnimationIr>>; 2],
    /// `busted`, played once blows break it, for a door whose
    /// `genericdoors` row names it (`Instance::busted`).
    pub busted: Option<Arc<AnimationIr>>,
    /// What its clips key beyond places and turns: its meshes' alpha.
    pub fx: Option<Arc<ModelFx>>,
}

/// A creature drawn in the scene: which object of the module it is (its
/// index in the area's instances), its body model, and what it is animated
/// with. A humanoid body (one with the humanoid skeleton the rig maps,
/// `asset_ja::rig::is_humanoid`: the bodies of appearance model type `B`,
/// and full bodies of type `F` such as the Sith soldiers') takes JA's
/// animations through the rig, its fingers held as its idle clip's first
/// frame holds them; any other plays its own KOTOR clips (reone's names: a
/// creature type `S` or `L` has its own `c…` set).
pub struct CreatureView {
    pub object: u32,
    pub body: usize,
    pub humanoid: bool,
    pub clips: CreatureClips,
    /// The models of the weapons in its right and left weapon slots.
    pub weapons: [Option<usize>; 2],
    /// Its sound set's death line, by its sound's name, and the clip.
    pub cry: Option<(String, audio::Clip)>,
    /// Its sound set (`soundset.2da` row), whose other lines are read
    /// when said (`ambience`).
    pub sound_set: Option<u16>,
    /// Its body's emitters (`Plays::Body`, as model `body`), if it has any.
    pub fx: Option<OwnFx>,
    /// How far its body leans with the floor (`creature::ground_tilt`).
    pub ground_tilt: f32,
    /// The alignment it was dressed for (`alignment_look`); none for a
    /// look without a head, which its alignment does not change.
    pub aligned: Option<creature::Aligned>,
    /// Its template is a hologram's (`CreatureLook::hologram`): it is drawn
    /// as one (`holo_view`).
    pub hologram: bool,
}

/// The KOTOR clips a creature's state picks, from its model or its
/// supermodels; `None` where the chain has none.
#[derive(Default)]
pub struct CreatureClips {
    pub pause: Option<Arc<AnimationIr>>,
    pub walk: Option<Arc<AnimationIr>>,
    pub run: Option<Arc<AnimationIr>>,
    pub die: Option<Arc<AnimationIr>>,
    pub dead: Option<Arc<AnimationIr>>,
    /// Getting up from its dead clip (KOTOR's `getupdead`: a knocked-out
    /// party member revived).
    pub getup: Option<Arc<AnimationIr>>,
    /// Its clips for scripts' animations, by `ANIMATION_…` constant
    /// (`acting::clips`).
    pub acted: Vec<(u16, Arc<AnimationIr>)>,
    /// The metres its walk and run cycles cover (`appearance.2da`): a
    /// humanoid's gait (`gait`), another's walk and run (`cast::Clips`).
    pub strides: [f32; 2],
    /// A creature model (type S or L, `creature::creature_model`): its own
    /// `c…` names, and no wield's ready stance in a fight.
    pub creature: bool,
}

/// Effects the scene draws each frame, facing the camera: JA's FX
/// primitives and the cgame's own. Places are Quake units in game space
/// (metres / `asset_ja::UNIT`); times are the clip's milliseconds, over
/// `period`, and each frame shows what the last client frame (`frame_ms`
/// apart) drew.
#[derive(Debug, Clone, Default)]
pub struct Effects {
    pub period_ms: f32,
    /// Client frames fall every `frame_ms` from `frame_origin_ms`.
    pub frame_ms: f32,
    pub frame_origin_ms: f32,
    pub items: Vec<Effect>,
    /// How each shader the items name blends (its texture is in
    /// `LoadedScene::textures` under the shader's name).
    pub blends: BTreeMap<String, EffectBlend>,
    /// The model a push's or pull's distortion is drawn with (JA's
    /// `models/weaphits/testboom.md3`): positions (units), texture
    /// coordinates, triangles.
    pub refraction_model: Option<EffectModel>,
}

/// A model drawn by an effect: positions (units), texture coordinates,
/// triangles.
pub type EffectModel = (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[u32; 3]>);

/// How an effect shader's (first) stage blends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectBlend {
    /// `blendFunc GL_ONE GL_ONE`, `rgbGen vertex`: the colour adds.
    Add,
    /// `blendFunc GL_SRC_ALPHA GL_ONE_MINUS_SRC_ALPHA`, `rgbGen vertex`
    /// (and so `alphaGen vertex`): tinted and faded by the vertex colour.
    Alpha,
    /// `blendFunc GL_DST_COLOR GL_SRC_COLOR`, no `rgbGen`: twice the texture
    /// times what is behind, so mid grey leaves it as it is, darker texels
    /// darken it and lighter ones brighten it (JA's burn marks); the vertex
    /// colour does not show.
    Modulate,
}

/// One effect.
#[derive(Debug, Clone)]
pub enum Effect {
    /// A primitive of JA's FX system (`asset_ja::fx`).
    Fx(asset_ja::fx::Spawned),
    /// A cgame puff (`LE_PUFF`): a sprite drifting sideways across the view
    /// at `speed` units/s (the view's left), growing from 8 to `radius` + 8
    /// and darkening to nothing over its life.
    Puff {
        start_ms: f32,
        end_ms: f32,
        origin: [f32; 3],
        speed: f32,
        radius: f32,
        rotation: f32,
        colour: [u8; 3],
        shader: String,
    },
    /// A push's or pull's distortion (JA renders it to a
    /// texture): the refraction model at the hand, facing the view, for
    /// `start_ms` to `end_ms`; `path` is the hand every client frame.
    Refraction {
        start_ms: f32,
        end_ms: f32,
        pull: bool,
        path: Vec<(f32, [f32; 3])>,
    },
}

/// A model shown only within some stretches of its clip.
#[derive(Debug, Clone)]
pub struct Shown {
    pub model: usize,
    /// (from, to) seconds.
    pub ranges: Vec<(f32, f32)>,
    /// The clip period the ranges repeat over.
    pub period: f32,
    /// For a model that moves on its own (a thrown saber): its root's
    /// transform in scene space at times (seconds), interpolated; empty to
    /// stay where it is placed.
    pub path: Vec<(f32, Matrix)>,
}

/// A saber blade: the hilt model it grows from, its bolt in the hilt's
/// space, size in metres, and the glow and core shader names (their
/// textures are in `LoadedScene::textures` under those names).
#[derive(Debug, Clone)]
pub struct BladeSpec {
    pub model: usize,
    pub origin: [f32; 3],
    pub dir: [f32; 3],
    pub length: f32,
    pub radius: f32,
    pub glow: String,
    pub core: String,
    /// The blade's length over the clip, (seconds, Quake units), linear
    /// between keys; empty for a blade always at `length`.
    pub extend: Vec<(f32, f32)>,
    /// The clip period `extend` and `trail` repeat over.
    pub period: f32,
    /// The blade's path for its trail, one key per client frame; empty for
    /// no trail.
    pub trail: Vec<TrailKey>,
    /// The trail's shader (its texture is in `LoadedScene::textures`).
    pub trail_texture: String,
    /// The trail's colour (0 to 1).
    pub trail_color: [f32; 3],
    /// (from, to) seconds of the period in which the blade is drawn; empty
    /// for always.
    pub shown: Vec<(f32, f32)>,
    /// Lit and trailed each frame as `scene::LiveBlades` says (a play
    /// scene's blades, from the snapshots), instead of by the clip.
    pub live: bool,
    /// The KOTOR blade texture whose colour the blade takes, for a colour
    /// JA has no blade of (`SaberColor::of_texture`); its glow and core are
    /// then JA's blue ones recoloured.
    pub own: Option<String>,
    /// The blade a mod ships with its hilt, drawn in place of JA's glow and
    /// core: its planes, core and glow meshes, in the hilt's space, with
    /// their own textures (`weapons::kotor_blades`). It grows and shrinks
    /// along the blade as JA's does and reaches as far. Empty for a blade
    /// drawn as JA's.
    pub meshes: Vec<asset_odyssey::model::MeshIr>,
    /// How far the shipped blade's core reaches from `origin` along `dir`
    /// (metres; its glow and planes when it has no core): it is drawn
    /// stretched along the blade so the core ends where the blade does.
    /// 0 for a blade drawn as JA's.
    pub reach: f32,
    /// For a crossguard's side blade, the main blade it is lit with (its
    /// index among the hilt's blades); `None` for a main blade
    /// (`weapons::kotor_blades`).
    pub follows: Option<usize>,
}

impl BladeSpec {
    /// A blade always at full length, untrailed, in `color` (its trail
    /// JA's `saberblur`), on model 0 for the caller to set.
    pub fn new(
        origin: [f32; 3],
        dir: [f32; 3],
        length: f32,
        radius: f32,
        color: SaberColor,
    ) -> BladeSpec {
        BladeSpec {
            model: 0,
            origin,
            dir,
            length,
            radius,
            glow: color.glow(),
            core: color.core(),
            extend: Vec::new(),
            period: 0.0,
            trail: Vec::new(),
            trail_texture: SaberColor::TRAIL.into(),
            trail_color: color.trail(),
            shown: Vec::new(),
            live: false,
            own: None,
            meshes: Vec::new(),
            reach: 0.0,
            follows: None,
        }
    }
}

/// JA's saber colours (its blade colours): each has its blade's glow and
/// core shaders and its trail's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaberColor {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
}

impl SaberColor {
    pub const ALL: [SaberColor; 6] = [
        SaberColor::Red,
        SaberColor::Orange,
        SaberColor::Yellow,
        SaberColor::Green,
        SaberColor::Blue,
        SaberColor::Purple,
    ];

    /// Every colour's trail shader.
    pub const TRAIL: &str = "gfx/effects/sabers/saberblur";

    /// The JA colour of a KOTOR blade texture, by the colour its name gives
    /// (`w_lsabre<colour>01`; both games' are blue, red, gold, yelo, gren,
    /// purp and turq, TSL's dgrn and silv too): JA has blue, red, yellow,
    /// green and purple. `None` for the others (gold, turquoise, dark green,
    /// silver: TSL's bronze, cyan, viridian and silver crystals, and any
    /// name we do not know), which take the texture's own colour on JA's
    /// blade (`weapons::own_colour`).
    pub fn of_texture(texture: &str) -> Option<SaberColor> {
        let texture = texture.to_ascii_lowercase();
        let name = texture.strip_prefix("w_lsabre").unwrap_or(&texture);
        [
            ("blue", SaberColor::Blue),
            ("red", SaberColor::Red),
            ("yelo", SaberColor::Yellow),
            ("gren", SaberColor::Green),
            ("purp", SaberColor::Purple),
        ]
        .into_iter()
        .find(|(colour, _)| name.starts_with(colour))
        .map(|(_, color)| color)
    }

    fn name(self) -> &'static str {
        match self {
            SaberColor::Red => "red",
            SaberColor::Orange => "orange",
            SaberColor::Yellow => "yellow",
            SaberColor::Green => "green",
            SaberColor::Blue => "blue",
            SaberColor::Purple => "purple",
        }
    }

    pub fn glow(self) -> String {
        format!("gfx/effects/sabers/{}_glow", self.name())
    }

    pub fn core(self) -> String {
        format!("gfx/effects/sabers/{}_line", self.name())
    }

    pub fn trail(self) -> [f32; 3] {
        let rgb = match self {
            SaberColor::Red => [255.0, 0.0, 0.0],
            SaberColor::Orange => [255.0, 64.0, 0.0],
            SaberColor::Yellow => [255.0, 255.0, 0.0],
            SaberColor::Green => [0.0, 255.0, 0.0],
            SaberColor::Blue => [0.0, 64.0, 255.0],
            SaberColor::Purple => [220.0, 0.0, 255.0],
        };
        rgb.map(|c| c / 255.0)
    }
}

/// Where a blade was at a moment (game space), and how long the trail
/// segment ending there lasts (0: none).
#[derive(Debug, Clone, Copy)]
pub struct TrailKey {
    pub time: f32,
    pub base: [f32; 3],
    pub tip: [f32; 3],
    pub life_ms: f32,
}

/// The ambient light (grey 0.2) of an area that sets none. Our chosen level.
const DEFAULT_AMBIENT: [f32; 3] = [0.2; 3];

fn game_of(key: &str) -> Result<Game, String> {
    Game::from_key(key)
        .filter(|game| *game != Game::JediAcademy)
        .ok_or("use k1 or tsl".into())
}

pub fn resolver(
    discovery: &Discovery,
    game: Game,
    module: Option<&str>,
) -> Result<Resolver, String> {
    crate::installs::open(discovery, game)?.resolver(module)
}

/// A texture's facts without decoding its pixels: TPC first, then TGA,
/// with a loose TXI replacing the TPC's own. Also whether it is a cube map
/// and how many flip-book frames it has.
#[cfg(feature = "probes")]
pub fn texture_facts(
    resolver: &Resolver,
    name: &str,
) -> Option<(asset_odyssey::texture::TextureFacts, bool, usize)> {
    let parsed = resolver.texture(name)?;
    let txi = texture_txi(Some(&parsed), |restype| resolver.read(name, restype));
    let alpha = parsed.has_alpha();
    let facts =
        asset_odyssey::texture::TextureFacts::from_txi(txi.as_ref().unwrap_or(&parsed.txi), alpha);
    let cube = parsed.cube && parsed.layers.len() == 6;
    let frames = if parsed.cube { 1 } else { parsed.layers.len() };
    Some((facts, cube, frames))
}

/// The texture the games would pick for a name: the highest layer's TPC or
/// TGA, TPC first within a layer (`Resolver::find_any`).
/// One that is there but does not read is warned of, once a name, and
/// drawn as a missing one. One larger than the GPU takes is reduced to fit
/// (`scene::fit_to_gpu`).
pub(crate) fn texture(resolver: &Resolver, name: &str) -> Option<TextureIr> {
    let parsed = resolver.read_texture(name)?;
    let decoded = (parsed.as_ref().ok()).and_then(|texture| {
        let txi = texture_txi(Some(texture), |restype| resolver.read(name, restype));
        TextureIr::from_texture(texture, txi.as_ref())
    });
    if decoded.is_none() {
        static WARNED: std::sync::Mutex<BTreeSet<String>> = std::sync::Mutex::new(BTreeSet::new());
        let mut warned = WARNED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if warned.insert(name.to_string()) {
            let why = parsed
                .err()
                .unwrap_or_else(|| "a level does not decode".into());
            warn!("texture {name} does not read, drawn as missing: {why}");
        }
    }
    let fitted =
        (decoded.as_ref()).and_then(|texture| crate::scene::fit_to_gpu(texture, Some(name)));
    fitted.or(decoded)
}

impl LoadedScene {
    /// The lightmap the game's data stores under `name`: the one a baked
    /// lightmap is drawn in the place of (`stored_lightmaps`), or the one
    /// the area draws.
    pub(crate) fn stored_lightmap(&self, name: &str) -> Option<&TextureIr> {
        stored_or_drawn(&self.stored_lightmaps, &self.textures, name)
    }
}

/// The lightmap the game's data stores under `name`: from `stored`, the
/// stored lightmaps a baked one is drawn in the place of, by name in lower
/// case, else from `drawn`, the area's textures.
pub(crate) fn stored_or_drawn<'a>(
    stored: &'a BTreeMap<String, TextureIr>,
    drawn: &'a BTreeMap<String, TextureIr>,
    name: &str,
) -> Option<&'a TextureIr> {
    stored
        .get(&name.to_ascii_lowercase())
        .or_else(|| drawn.get(name))
}

/// The lines `<lightmap> <number>` of file `file` in the bake's set
/// `folder`, by the lightmap's name in lower case.
fn baked_numbers(folder: &std::path::Path, file: &str) -> BTreeMap<String, f32> {
    let Ok(text) = std::fs::read_to_string(folder.join(file)) else {
        return BTreeMap::new();
    };
    (text.lines())
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (name, fill) = line.split_once(' ')?;
            Some((name.to_ascii_lowercase(), fill.trim().parse().ok()?))
        })
        .collect()
}

/// One folder of `baked_folders`, read once.
#[derive(Debug)]
struct BakeFolder {
    path: std::path::PathBuf,
    /// Its baked lightmaps, by name in lower case. A lightmap's name holds
    /// its area's, so two areas never share one.
    lightmaps: BTreeMap<String, std::path::PathBuf>,
    /// How much of its stored light each baked lightmap takes: `fill.txt`,
    /// which `kjk detail-bake --fill-stored` writes.
    fills: BTreeMap<String, f32>,
    /// The least share of its stored linear light each texel of a baked
    /// lightmap shows: `floor.txt`. The bake writes it for a module with the
    /// stored fill, so a wall that no lamp reaches is not black (`FILL_FLOOR`
    /// in `crates/launcher/src/detail_bake/gpu.rs`).
    floors: BTreeMap<String, f32>,
    /// Whether the bake asks for its rooms' surfaces with no lightmap to be
    /// dark (`unlit.txt`, `dark_unlit`).
    unlit: bool,
    /// The lightmaps the set shows from the player's own stored lightmaps
    /// at their stored light, by name in lower case
    /// (`stored_fill::STORED_FILE`). It holds no PNG of them.
    stored: BTreeSet<String>,
    /// Its fingerprint (`bake_fingerprint::FILE`). `None` for a set baked
    /// before the baker wrote one, which the game shows as it is. The reason
    /// for a file that does not read, and the game then shows none of it.
    stamp: Option<Result<crate::bake_fingerprint::Fingerprint, String>>,
}

/// Every folder of `baked_folders`, in the list's order.
static BAKE_FOLDERS: std::sync::LazyLock<Vec<BakeFolder>> = std::sync::LazyLock::new(|| {
    let mut folders = Vec::new();
    for path in baked_folders() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            warn!("no folder of baked lightmaps at {}", path.display());
            continue;
        };
        let mut lightmaps = BTreeMap::new();
        for file in entries.flatten().map(|entry| entry.path()) {
            if file.extension().is_some_and(|extension| extension == "png")
                && let Some(name) = file.file_stem().and_then(|name| name.to_str())
            {
                lightmaps.insert(name.to_ascii_lowercase(), file);
            }
        }
        let stamp = crate::bake_fingerprint::read(&path);
        match &stamp {
            None => warn!(
                "the bake in {} has no {}, so the game cannot check that it was made from the game's files, and shows it as it is",
                path.display(),
                crate::bake_fingerprint::FILE
            ),
            Some(Err(why)) => warn!(
                "{why}, so the game shows none of the bake in {}",
                path.display()
            ),
            Some(Ok(_)) => {}
        }
        folders.push(BakeFolder {
            lightmaps,
            fills: baked_numbers(&path, "fill.txt"),
            floors: baked_numbers(&path, "floor.txt"),
            unlit: path.join("unlit.txt").is_file(),
            stored: crate::stored_fill::stored_names(&path),
            stamp,
            path,
        });
    }
    folders
});

/// How much of a folder of `BAKE_FOLDERS` the game takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FolderUse {
    /// The lightmaps, their fills, the probes and the placeables' light.
    Whole,
    /// Everything but the placeables' light.
    Lightmaps,
    /// Nothing. The area is drawn as if no bake existed for it.
    Nothing,
}

/// What the game takes of a folder with a fingerprint for one module's
/// area (`choose_bakes`).
#[derive(Debug, Clone)]
struct Decision {
    using: FolderUse,
    /// How the set fits the player's files lightmap by lightmap, for a set
    /// with exact lines (`bake_fingerprint::fit`).
    fit: Option<Arc<crate::bake_fingerprint::Fit>>,
}

/// The decision for each folder with a fingerprint, by its number in
/// `BAKE_FOLDERS` and the module in lower case, made when the module's
/// area first loads (`choose_bakes`). The module is part of the key because
/// some modules share an area's name, and their files can differ.
static FOLDER_USES: std::sync::Mutex<BTreeMap<(usize, String), Decision>> =
    std::sync::Mutex::new(BTreeMap::new());

/// A folder of `BAKE_FOLDERS` that the game takes something of for an area.
#[derive(Debug, Clone)]
struct Used<'a> {
    folder: &'a BakeFolder,
    using: FolderUse,
    /// How the set fits the player's files, for a set with exact lines.
    fit: Option<Arc<crate::bake_fingerprint::Fit>>,
}

/// The folders of `BAKE_FOLDERS` the game takes something of in module
/// `module`, in the list's order, each with its use. A folder without a
/// fingerprint is taken whole. One with a fingerprint is taken as
/// `choose_bakes` decided for this module, and not at all before the
/// module's area has loaded.
fn used_folders(module: &str) -> Vec<Used<'static>> {
    let module = module.to_ascii_lowercase();
    let uses = FOLDER_USES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    (BAKE_FOLDERS.iter().enumerate())
        .filter_map(|(index, folder)| {
            let (using, fit) = match &folder.stamp {
                None => (FolderUse::Whole, None),
                Some(Err(_)) => return None,
                Some(Ok(_)) => {
                    let decision = uses.get(&(index, module.clone()))?;
                    (decision.using, decision.fit.clone())
                }
            };
            (using != FolderUse::Nothing).then_some(Used { folder, using, fit })
        })
        .collect()
}

/// A stamped set's verdict over the player's files, and its fit when it has
/// exact lines.
#[derive(Debug, Clone)]
struct Judged {
    verdict: crate::bake_fingerprint::Verdict,
    fit: Option<crate::bake_fingerprint::Fit>,
}

/// Judges the set stamped `stamp` over the player's files `current`. A set
/// without exact lines is judged by `rooms` and `placeables` alone
/// (`bake_fingerprint::verdict`). A set with them is judged lightmap by
/// lightmap (`bake_fingerprint::fit`). It is refused when no baked lightmap
/// matches, and the area is then drawn with the game's own lighting. A set
/// that fits in full ranks as `Whole`. A set that fits in part ranks as
/// `LightmapsOnly`, but the game still takes the light of each placeable
/// that matches.
fn judge(
    stamp: &crate::bake_fingerprint::Fingerprint,
    current: &crate::bake_fingerprint::Current,
) -> Judged {
    use crate::bake_fingerprint::Verdict;
    match crate::bake_fingerprint::fit(stamp, current) {
        None => Judged {
            verdict: crate::bake_fingerprint::verdict(stamp, &current.print),
            fit: None,
        },
        Some(Err(why)) => Judged {
            verdict: Verdict::Refused(why),
            fit: None,
        },
        Some(Ok(fit)) if !fit.baked() => Judged {
            verdict: Verdict::Refused(
                "no baked lightmap is drawn on the room triangles the bake was made over".into(),
            ),
            fit: None,
        },
        Some(Ok(fit)) => Judged {
            verdict: if fit.differing.is_empty() && !fit.placeables_differ {
                Verdict::Whole
            } else {
                Verdict::LightmapsOnly
            },
            fit: Some(fit),
        },
    }
}

/// Decides, when area `area` of module `module` loads under the bake, which
/// of the folders stamped for it the game takes, by comparing each one's
/// fingerprint with the fingerprint of the files `resolver` reads
/// (`bake_fingerprint`, `judge`).
///
/// - A folder whose rooms or format differ gives nothing. The area is drawn
///   with the game's own lightmaps, as if no bake existed for it.
/// - A folder without exact lines whose rooms match but whose placeables
///   differ gives its lightmaps, and the placeables keep the light probes.
/// - A folder with exact lines gives each baked lightmap that matches and
///   the light of each placeable that matches. The game draws the other
///   lightmaps of the rooms at their stored light (`lightmap_source`).
/// - Of two folders for the area, the game takes the one that matches the
///   most, and of two that match as much, the later in the list. An
///   unpatched and a patched bake of one area can then ship side by side.
///
/// The fingerprint is computed once for each module's area in a run.
fn choose_bakes(resolver: &Resolver, area: &asset_odyssey::module::AreaIr, module: &str) {
    use crate::bake_fingerprint::Verdict;
    let module = module.to_ascii_lowercase();
    let stamped: Vec<(usize, &crate::bake_fingerprint::Fingerprint)> = (BAKE_FOLDERS.iter())
        .enumerate()
        .filter_map(|(index, folder)| match &folder.stamp {
            Some(Ok(stamp)) if stamp.area.eq_ignore_ascii_case(&area.resref) => {
                Some((index, stamp))
            }
            _ => None,
        })
        .collect();
    let mut uses = FOLDER_USES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if (stamped.iter()).all(|(index, _)| uses.contains_key(&(*index, module.clone()))) {
        return;
    }
    let started = std::time::Instant::now();
    let current = crate::bake_fingerprint::current(resolver, area);
    let name = current.print.area.as_str();
    bevy::log::info!(
        "{name}: the game's files have the fingerprint rooms {} placeables {} (computed in {:.0} ms)",
        current.print.rooms,
        current.print.placeables,
        started.elapsed().as_secs_f64() * 1000.0
    );
    let judged: Vec<(usize, Judged)> = (stamped.iter())
        .map(|(index, stamp)| (*index, judge(stamp, &current)))
        .collect();
    let verdicts: Vec<(usize, Verdict)> = (judged.iter())
        .map(|(index, judged)| (*index, judged.verdict.clone()))
        .collect();
    let chosen = chosen_bake(&verdicts);
    for (index, Judged { verdict, fit }) in judged {
        let path = BAKE_FOLDERS[index].path.display();
        if let Some(fit) = fit.filter(|_| Some(index) == chosen) {
            report_fit(name, &path.to_string(), &fit);
            let decision = Decision {
                using: FolderUse::Whole,
                fit: Some(Arc::new(fit)),
            };
            uses.insert((index, module.clone()), decision);
            continue;
        }
        let using = match (&verdict, Some(index) == chosen) {
            (Verdict::Whole, true) => {
                bevy::log::info!(
                    "{name}: the bake in {path} was made from the game's files, and the game shows all of it"
                );
                FolderUse::Whole
            }
            (Verdict::LightmapsOnly, true) => {
                warn!(
                    "{name}: the bake in {path} was made with other placeables than the game's files have, so the game shows its lightmaps, and the placeables take their light from the light probes"
                );
                FolderUse::Lightmaps
            }
            (Verdict::Refused(why), _) => {
                warn!(
                    "{name}: the game shows its own lightmaps and nothing of the bake in {path}, because {why}"
                );
                FolderUse::Nothing
            }
            (_, false) => {
                bevy::log::info!(
                    "{name}: the bake in {path} is not used, because a folder later in the list or one that matches better holds a bake of this area"
                );
                FolderUse::Nothing
            }
        };
        uses.insert((index, module.clone()), Decision { using, fit: None });
    }
}

/// Says in the log how the chosen set in folder `path` fits the files of
/// area `area`: how many of its baked lightmaps match, each lightmap that
/// differs with the room models that draw it, and whether its placeables
/// differ.
fn report_fit(area: &str, path: &str, fit: &crate::bake_fingerprint::Fit) {
    let total = fit.matching.len() + fit.differing.len();
    bevy::log::info!(
        "{area}: the bake in {path} fits {} of its {total} baked lightmaps to the game's files, and the game shows those",
        fit.matching.len()
    );
    for (lightmap, rooms) in &fit.differing {
        let drawn_by = if rooms.is_empty() {
            "No room model draws it any more.".to_string()
        } else {
            format!(
                "These room models draw it: {}.",
                rooms.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        };
        warn!(
            "{area}: the room triangles of lightmap {lightmap} differ from the ones the bake in {path} was made over, so the game draws it at its stored light. {drawn_by}"
        );
    }
    if fit.placeables_differ {
        warn!(
            "{area}: some lit placeables differ from the ones the bake in {path} was made with. {} of them match and take the bake's light, and the others take their light from the light probes",
            fit.placeables.len()
        );
    }
}

/// Of the folders stamped for one area, with each one's verdict in the
/// list's order, the one the game takes: the last that matches whole, else
/// the last whose lightmaps alone match. `None` when every one is refused.
fn chosen_bake(verdicts: &[(usize, crate::bake_fingerprint::Verdict)]) -> Option<usize> {
    use crate::bake_fingerprint::Verdict;
    let last = |wanted: Verdict| {
        (verdicts.iter().rev())
            .find(|(_, verdict)| *verdict == wanted)
            .map(|(index, _)| *index)
    };
    last(Verdict::Whole).or_else(|| last(Verdict::LightmapsOnly))
}

/// Whether the used folder `folder` holds a bake of area `area` (its
/// resref) of module `module`. A folder with a fingerprint names its area
/// there. A folder without one, which only a developer's variable lists, is
/// known by its own name or the name of the folder over it, which the bake
/// names after the area (`bake/<round>/<area>/b`). An installed download's
/// set is the area's folder itself. The module's name counts too, for a
/// folder named after it.
fn holds_area(folder: &BakeFolder, area: &str, module: &str) -> bool {
    match &folder.stamp {
        Some(Ok(stamp)) => stamp.area.eq_ignore_ascii_case(area),
        Some(Err(_)) => false,
        None => [folder.path.parent(), Some(folder.path.as_path())]
            .into_iter()
            .flatten()
            .filter_map(|path| path.file_name()?.to_str())
            .any(|name| name.eq_ignore_ascii_case(area) || name.eq_ignore_ascii_case(module)),
    }
}

/// Whether area `area` (its resref) has a bake the game shows: a folder of
/// `used` holds one (`choose_bakes`). A folder without a fingerprint counts
/// for every area, as it is shown whole wherever its lightmaps' names
/// match, and only a developer's variable lists one.
fn takes_bake(used: &[Used], area: &str) -> bool {
    (used.iter()).any(|Used { folder, .. }| match &folder.stamp {
        None => true,
        Some(Ok(stamp)) => stamp.area.eq_ignore_ascii_case(area),
        Some(Err(_)) => false,
    })
}

/// Whether area `area` of module `module` has a used bake that asks for its
/// rooms' surfaces with no lightmap to be dark: the bake's set holds
/// `unlit.txt`. `bake.sh` writes the file for a module whose entry in
/// `data/lights/bake.toml` has `dark_unlit = true`. Nar Shaddaa's tunnels
/// (305nar) have no lightmap, and the game showed them at half of full
/// light, which the owner called too bright (21 November 2026). The area is
/// matched by its resref (`holds_area`), so a KOTOR 1 area such as `m13aa`
/// of module `danm13` is found too. `used` holds the module's used folders.
fn dark_unlit(used: &[Used], area: &str, module: &str) -> bool {
    (used.iter()).any(|Used { folder, .. }| folder.unlit && holds_area(folder, area, module))
}

/// Under the baked trial, the probes the bake wrote for the rooms of
/// `rooms` (`probes.txt` in a set of `baked_folders`): each one's room, its
/// place (game space) and the light on its six sides. `None` when no set
/// holds a probe of these rooms. A set baked before the bake wrote probes
/// has no file, and the game makes its probes from the lightmaps.
///
/// Under `KJK_PROBE_SUN=on` the probes come from `probes-sun.txt` instead,
/// with their light without the sun and their share of it, and the second
/// part is the sun (`light_probes::ProbeSun`).
///
/// A probe whose room takes its fill from the stored light holds its light
/// without it, and the game adds the fill from the player's own stored
/// lightmap (`stored_fill`). A file of an older bake holds that fill from
/// the games' own lightmaps and has no `format` line. The game takes none
/// of it and makes its probes from the lightmaps.
fn baked_probes(
    (stored, used): (Stored, &[Used]),
    rooms: &[RoomView],
    area_baked: bool,
) -> (
    Option<Vec<crate::light_probes::BakedProbe>>,
    Option<crate::light_probes::ProbeSun>,
) {
    // The owner preferred the probes made from the lightmaps, so the bake's
    // probes are read only under `KJK_BAKED_PROBES=on`.
    if !area_baked || std::env::var("KJK_BAKED_PROBES").map_or(true, |value| value != "on") {
        return (None, None);
    }
    let live_sun = std::env::var("KJK_PROBE_SUN").is_ok_and(|value| value == "on");
    let names: std::collections::HashSet<String> = rooms
        .iter()
        .map(|room| room.name.to_ascii_lowercase())
        .collect();
    let mut probes = Vec::new();
    let mut sun = None;
    for Used { folder, .. } in used {
        let file = if live_sun {
            "probes-sun.txt"
        } else {
            "probes.txt"
        };
        let Ok(text) = std::fs::read_to_string(folder.path.join(file)) else {
            continue;
        };
        let mut lines = text
            .lines()
            .filter(|line| !line.starts_with('#'))
            .peekable();
        let mut format = None;
        let mut maps = Vec::new();
        while let Some(line) = lines.peek() {
            if let Some(number) = line.strip_prefix("format ") {
                format = number.trim().parse::<u32>().ok();
            } else if let Some(map) = crate::stored_fill::StoredMap::parse(line) {
                maps.push(map);
            } else {
                break;
            }
            lines.next();
        }
        if format != Some(crate::bake_fingerprint::FORMAT) {
            warn!(
                "{} is of an older bake (format {}, where the game reads format {}), so the game makes its probes from the lightmaps",
                folder.path.join(file).display(),
                format.map_or("none".to_string(), |format| format.to_string()),
                crate::bake_fingerprint::FORMAT
            );
            continue;
        }
        let mut fills = StoredFills::new(stored, folder);
        for line in lines {
            let mut words = line.split_whitespace();
            let Some(first) = words.next() else {
                continue;
            };
            if live_sun && first == "sun" {
                let numbers: Vec<f32> = words.filter_map(|word| word.parse().ok()).collect();
                if numbers.len() == 9 {
                    sun = Some(crate::light_probes::ProbeSun {
                        way: bevy::math::Vec3::from_slice(&numbers[..3]).normalize_or_zero(),
                        colour: bevy::math::Vec3::from_slice(&numbers[3..6]),
                        curve: bevy::math::Vec3::from_slice(&numbers[6..9]),
                    });
                }
                continue;
            }
            let room = first.to_ascii_lowercase();
            if !names.contains(&room) {
                continue;
            }
            let words: Vec<&str> = words.collect();
            // The stored texel of the probe's fill, the last word where it
            // has one.
            let (words, stored) = match words.split_last() {
                Some((last, rest)) if last.starts_with('@') => {
                    let Some((map, texel)) = crate::stored_fill::parse_word(last) else {
                        continue;
                    };
                    let Some(map) = maps.get(map) else {
                        continue;
                    };
                    (rest, Some((map, texel)))
                }
                _ => (&words[..], None),
            };
            let numbers: Vec<f32> = words.iter().filter_map(|word| word.parse().ok()).collect();
            let (share, light) = match (live_sun, numbers.len()) {
                (true, 22) => (numbers[3], &numbers[4..]),
                (false, 21) => (0.0, &numbers[3..]),
                _ => continue,
            };
            let at = bevy::math::Vec3::from_slice(&numbers[..3]);
            let parts = stored.and_then(|(map, texel)| fills.parts(map, texel));
            let light = std::array::from_fn(|side| {
                let light = bevy::math::Vec3::from_slice(&light[3 * side..3 * side + 3]);
                match parts {
                    // The live sun's probes hold linear light before the
                    // curve, and the others a level as the lightmaps store
                    // it.
                    Some((stored, fill, floor)) if live_sun => {
                        crate::stored_fill::with_fill_linear(light, stored, fill, floor)
                    }
                    Some((stored, fill, floor)) => {
                        crate::stored_fill::with_fill(light, stored, fill, floor)
                    }
                    None => light,
                }
            });
            probes.push(crate::light_probes::BakedProbe {
                room,
                at,
                light,
                sun: share,
            });
        }
        fills.report("probes");
    }
    if probes.is_empty() {
        return (None, None);
    }
    (Some(probes), sun)
}

/// Where an area's load finds its stored lightmaps: the resolver, and the
/// stored lightmaps that baked ones replaced (`baked_lightmaps`).
#[derive(Clone, Copy)]
struct Stored<'a> {
    resolver: &'a Resolver,
    lightmaps: &'a BTreeMap<String, TextureIr>,
}

/// The stored lightmaps that the probes and placeables of a bake's set read
/// their fill from (`stored_fill`), each decoded once, with the set's fills
/// and floors.
struct StoredFills<'a> {
    stored: Stored<'a>,
    folder: &'a BakeFolder,
    texels: BTreeMap<String, Option<crate::stored_fill::StoredTexels>>,
    /// How many places named a lightmap that the game could not read.
    missed: usize,
}

impl<'a> StoredFills<'a> {
    fn new(stored: Stored<'a>, folder: &'a BakeFolder) -> StoredFills<'a> {
        StoredFills {
            stored,
            folder,
            texels: BTreeMap::new(),
            missed: 0,
        }
    }

    /// The stored level at texel `texel` of `map`, and the fill and the
    /// floor the set gives that lightmap. `None` where the set gives it
    /// neither, or where the stored lightmap does not read.
    fn parts(
        &mut self,
        map: &crate::stored_fill::StoredMap,
        texel: usize,
    ) -> Option<(Vec3, Option<f32>, Option<f32>)> {
        let fill = self.folder.fills.get(&map.name).copied();
        let floor = self.folder.floors.get(&map.name).copied();
        if fill.is_none() && floor.is_none() {
            return None;
        }
        let (u, v) = map.uv(texel)?;
        let Stored {
            resolver,
            lightmaps,
        } = self.stored;
        let texels = self.texels.entry(map.name.clone()).or_insert_with(|| {
            // The rooms' lightmaps have loaded by now (`baked_lightmaps`).
            (lightmaps.get(&map.name.to_ascii_lowercase()).cloned())
                .or_else(|| {
                    let parsed = resolver.read_texture(&map.name)?.ok()?;
                    let txi =
                        texture_txi(Some(&parsed), |restype| resolver.read(&map.name, restype));
                    TextureIr::from_texture(&parsed, txi.as_ref())
                })
                .and_then(|texture| crate::stored_fill::StoredTexels::of(&texture))
        });
        let Some(texels) = texels else {
            self.missed += 1;
            return None;
        };
        Some((texels.at(u, v), fill, floor))
    }

    /// One warning for the places whose stored lightmap did not read.
    fn report(&self, what: &str) {
        if self.missed > 0 {
            warn!(
                "{} {what} of the bake in {} take no fill, because their stored lightmap does not read",
                self.missed,
                self.folder.path.display()
            );
        }
    }
}

/// Each lit placeable of the scene, by its object: its model's number in
/// the scene, its name and which of its meshes the bake may light.
type LitPlaceables = BTreeMap<u32, (usize, String, Arc<Vec<bool>>)>;

/// The placeables of `lit` by the number under which a set's
/// `placeables.txt` holds their light, each with its own object and its
/// model's name. A set without exact lines names each placeable by its own
/// number. A set with them names a placeable by the number of the baked
/// placeable it matches (`Fit::placeables`), so a placeable that a mod adds
/// before it does not take its light away. A placeable that matches none
/// is left out and keeps its usual light.
fn light_numbers<'a>(
    lit: &'a LitPlaceables,
    fit: Option<&crate::bake_fingerprint::Fit>,
) -> BTreeMap<u32, (u32, &'a str)> {
    (lit.iter())
        .filter_map(|(object, (_, name, _))| {
            let baked = match fit {
                None => *object,
                Some(fit) => u32::try_from(*fit.placeables.get(&(*object as usize))?).ok()?,
            };
            Some((baked, (*object, name.as_str())))
        })
        .collect()
}

/// Under the baked trial, the baked light of the placeables in `lit` that
/// the sets of `used` hold for area `area`, by each one's model in the
/// scene. `lit` gives each placeable with a mesh the bake may light its
/// model's number and name and which of its meshes those are, by its object
/// (`placeable_light::bends`). A skinned or dangling mesh keeps the probes,
/// whatever the set holds. Empty under `KJK_PLACEABLE_LIGHT=probes`.
fn baked_placeables(
    (stored, used): (Stored, &[Used]),
    area: &str,
    lit: &LitPlaceables,
    area_baked: bool,
) -> BTreeMap<usize, Arc<crate::placeable_light::BakedPlaceable>> {
    let mut found = BTreeMap::new();
    if !area_baked || !crate::placeable_light::baked_light() || lit.is_empty() {
        return found;
    }
    // A set without exact lines whose placeables are not the game's gives
    // none of their light (`choose_bakes`). Only the set chosen for this
    // area is read. A set without a fingerprint, which only a developer's
    // variable lists, is read for every area.
    let used = (used.iter()).filter(|Used { folder, using, .. }| {
        *using == FolderUse::Whole
            && match &folder.stamp {
                Some(Ok(stamp)) => stamp.area.eq_ignore_ascii_case(area),
                _ => true,
            }
    });
    for Used { folder, fit, .. } in used {
        let numbers = light_numbers(lit, fit.as_deref());
        let names: BTreeMap<u32, &str> = (numbers.iter())
            .map(|(baked, (_, name))| (*baked, *name))
            .collect();
        let mut fills = StoredFills::new(stored, folder);
        let mut fill = |map: &crate::stored_fill::StoredMap, texel: usize, light: [f32; 3]| {
            let light = Vec3::from_array(light);
            (fills.parts(map, texel))
                .map_or(light, |(stored, fill, floor)| {
                    crate::stored_fill::with_fill(light, stored, fill, floor)
                })
                .to_array()
        };
        let read = crate::placeable_light::read(&folder.path, area, &names, &mut fill);
        fills.report("placeables");
        for (baked, mut light) in read {
            let Some((object, _)) = numbers.get(&baked) else {
                continue;
            };
            if let Some((model, _, meshes)) = lit.get(object) {
                for (mesh, kept) in light.meshes.iter_mut().zip(meshes.iter()) {
                    if !kept {
                        *mesh = None;
                    }
                }
                found.insert(*model, Arc::new(light));
            }
        }
    }
    bevy::log::info!(
        "{area}: {} placeables have a mesh the bake may light, and {} of them take the bake's light",
        lit.len(),
        found.len()
    );
    found
}

/// The folders of baked lightmaps. When the player chose the baked
/// lightmaps in the options and `KJK_FITTED_LIGHTS` does not say otherwise,
/// the list starts with the installed download's area folders
/// (`lighting_bakes::folders`). `KJK_DETAIL_BAKE` names one area's
/// folder. `KJK_DETAIL_BAKES` names a file with one folder a line, for a
/// run that goes through many areas (`walk::step`). A line that starts
/// with `#` is a comment. The variables' folders come later in the list,
/// so they win over the download's for an area both hold (`choose_bakes`).
fn baked_folders() -> Vec<std::path::PathBuf> {
    let mut folders = if crate::lighting_bakes::chosen() && crate::fitted_lights::baked_anywhere() {
        crate::lighting_bakes::folders()
    } else {
        Vec::new()
    };
    if !folders.is_empty() {
        bevy::log::info!(
            "the baked lightmaps of the lighting download: {} areas",
            folders.len()
        );
    }
    folders.extend(
        std::env::var_os("KJK_DETAIL_BAKE")
            .filter(|folder| !folder.is_empty())
            .map(std::path::PathBuf::from),
    );
    if let Some(list) = std::env::var_os("KJK_DETAIL_BAKES").filter(|list| !list.is_empty()) {
        match std::fs::read_to_string(&list) {
            Ok(text) => folders.extend(
                (text.lines())
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(std::path::PathBuf::from),
            ),
            Err(error) => warn!("KJK_DETAIL_BAKES: {}: {error}", list.to_string_lossy()),
        }
    }
    folders
}

/// Adds `fill` times the stored lightmap's linear light to a baked
/// lightmap. A room with no lamp that the level art shows was lit by lamps
/// the games never shipped, and its stored lightmap is all that is left of
/// them. The bake holds the room's lamps alone, so that its files carry
/// nothing of the game's own lightmaps, and the stored light comes from the
/// player's files here.
///
/// The numbers are the bake's (`stored_fill`).
/// With `floor` the stored light times `fill` is the least the texel shows,
/// and not added (`BAKED_FLOORS`).
fn add_stored_fill(picture: &mut image::RgbaImage, stored: &TextureIr, fill: f32, floor: bool) {
    use crate::stored_fill::{GAIN, GAMMA, level_of, linear_of};
    let Some(texels) = crate::stored_fill::StoredTexels::of(stored) else {
        return;
    };
    let (w, h) = (picture.width() as usize, picture.height() as usize);
    for y in 0..h {
        for x in 0..w {
            // The stored light here, read between the four nearest texels.
            let u = (x as f32 + 0.5) / w as f32;
            let v = (y as f32 + 0.5) / h as f32;
            let added = texels.at(u, v).powf(GAMMA) * fill;
            // The baked level back to linear light, with its easing undone.
            let pixel = picture.get_pixel_mut(x as u32, y as u32);
            let level = Vec3::new(
                f32::from(pixel[0]),
                f32::from(pixel[1]),
                f32::from(pixel[2]),
            ) / 255.0
                * GAIN;
            let light = if floor {
                linear_of(level).max(added)
            } else {
                linear_of(level) + added
            };
            // And the sum eased again, as the bake eases it.
            let level = level_of(light);
            let bytes = (level / GAIN).clamp(Vec3::ZERO, Vec3::ONE) * 255.0;
            pixel[0] = bytes.x.round() as u8;
            pixel[1] = bytes.y.round() as u8;
            pixel[2] = bytes.z.round() as u8;
        }
    }
}

/// Puts each baked lightmap of the folders `used` in the place of the
/// stored lightmap of its name in `textures` (`detail_baked`), and gives
/// back the stored lightmaps it replaced, by name in lower case.
/// `room_lightmaps` names, in lower case, the lightmaps that the area's
/// room meshes draw.
fn baked_lightmaps(
    used: &[Used],
    room_lightmaps: &BTreeSet<String>,
    textures: &mut BTreeMap<String, TextureIr>,
) -> BTreeMap<String, TextureIr> {
    let mut stored = BTreeMap::new();
    if used.is_empty() {
        return stored;
    }
    for (name, texture) in textures.iter_mut() {
        let Some(baked) = detail_baked(used, room_lightmaps, name, texture) else {
            continue;
        };
        let baked = crate::scene::fit_to_gpu(&baked, Some(name)).unwrap_or(baked);
        stored.insert(name.to_ascii_lowercase(), std::mem::replace(texture, baked));
    }
    stored
}

/// Where the picture of a lightmap of the area comes from (`lightmap_source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Picture<'a> {
    /// The PNG the bake made of it, with the folder's fill and floor.
    Png(&'a std::path::Path),
    /// The player's stored lightmap at half its light, because the folder
    /// names it in `stored.txt`. The folder's fill and floor apply as to a
    /// PNG.
    Stored,
    /// The player's stored lightmap at half its light, with no fill and no
    /// floor. This is a room lightmap whose PNG does not fit the player's
    /// rooms, or one that the bake never saw.
    FallBack,
}

/// Where the picture of lightmap `key` (in lower case) comes from: the last
/// folder of `used` that knows it, and how. `None` where the game keeps the
/// stored lightmap as it is.
///
/// A folder with exact lines judges only the lightmaps of the room meshes
/// (`room_lightmaps`). The shader doubles every lightmapped room surface of
/// a baked area, because a PNG is stored at half its level. A room
/// lightmap that the game cannot show from a PNG is therefore drawn from
/// its stored lightmap at half its light, so that it shows its stored
/// light. The fill and the floor were made for the PNG, so they do not
/// apply to it. A backdrop the bake knew and left alone, and did not name
/// in `stored.txt`, is kept as it is, so it shows at twice its stored
/// light, as the owner approved.
fn lightmap_source<'a>(
    used: &[Used<'a>],
    room_lightmaps: &BTreeSet<String>,
    key: &str,
) -> Option<(&'a BakeFolder, Picture<'a>)> {
    let room = room_lightmaps.contains(key);
    (used.iter().rev()).find_map(|Used { folder, fit, .. }| {
        let folder: &'a BakeFolder = folder;
        let fit = fit.as_deref().filter(|_| room);
        if let Some(path) = folder.lightmaps.get(key) {
            let fits = fit.is_none_or(|fit| fit.matching.contains(key));
            return Some((
                folder,
                if fits {
                    Picture::Png(path)
                } else {
                    Picture::FallBack
                },
            ));
        }
        if folder.stored.contains(key) {
            return Some((folder, Picture::Stored));
        }
        fit.filter(|fit| !fit.backdrops.contains(key))
            .map(|_| (folder, Picture::FallBack))
    })
}

/// The picture the game draws for the stored lightmap `texture` of name
/// `name`, from the folders `used` (`lightmap_source`). `None` where the
/// game keeps the stored lightmap. A PNG is the larger lightmap
/// `kjk detail-bake` made (`docs/LIGHTING-PLAN.md`, step 10). A picture made
/// from the stored lightmap is at its stored light (`stored_fill::backdrop`).
/// The picture keeps the stored one's facts and has one level, because a
/// smaller level would mix the light of faces that lie side by side on the
/// lightmap.
fn detail_baked(
    used: &[Used],
    room_lightmaps: &BTreeSet<String>,
    name: &str,
    texture: &TextureIr,
) -> Option<TextureIr> {
    let key = name.to_ascii_lowercase();
    let (folder, source) = lightmap_source(used, room_lightmaps, &key)?;
    if texture.cube || texture.frames > 1 {
        return None;
    }
    let picture = match source {
        Picture::Png(path) => image::open(path)
            .map(image::DynamicImage::into_rgba8)
            .map_err(|error| format!("baked lightmap {} does not read: {error}", path.display())),
        Picture::Stored | Picture::FallBack => crate::stored_fill::backdrop(texture)
            .and_then(|bytes| image::RgbaImage::from_raw(texture.width, texture.height, bytes))
            .ok_or_else(|| {
                format!(
                    "stored lightmap {name} does not decode for the bake in {}",
                    folder.path.display()
                )
            }),
    };
    match picture {
        Ok(mut picture) => {
            if source != Picture::FallBack {
                if let Some(fill) = folder.fills.get(&key) {
                    add_stored_fill(&mut picture, texture, *fill, false);
                }
                if let Some(share) = folder.floors.get(&key) {
                    add_stored_fill(&mut picture, texture, *share, true);
                }
            }
            Some(TextureIr {
                width: picture.width(),
                height: picture.height(),
                encoding: asset_odyssey::texture::Encoding::Rgba8,
                mips: vec![picture.into_raw()],
                ..texture.clone()
            })
        }
        Err(why) => {
            warn!("{why}");
            None
        }
    }
}

pub fn model(resolver: &Resolver, name: &str) -> Result<Option<ModelIr>, String> {
    Ok(parsed(resolver, name)?.map(|parsed| ModelIr::from_model(&parsed)))
}

/// `model`'s own effects (`ModelFx::read`) with what its supermodel
/// chain's clips key on its nodes: a creature's body plays its chain's
/// clips (the welding droid's `weld` is its supermodel's).
/// A supermodel that does not read ends the chain there.
pub fn fx_through_chain(resolver: &Resolver, model: &Model) -> ModelFx {
    let mut fx = ModelFx::read(model);
    let mut seen = vec![model.name.to_ascii_lowercase()];
    let mut next = model.supermodel.clone();
    while let Some(name) = next
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty() && name != "null" && !seen.contains(name))
    {
        let parent = match parsed(resolver, &name) {
            Ok(Some(parent)) => parent,
            Ok(None) => break,
            Err(why) => {
                warn!("{}: supermodel {name} is not read: {why}", model.name);
                break;
            }
        };
        seen.push(name);
        fx.add_clips(&parent);
        next = parent.supermodel.clone();
    }
    fx
}

/// A model's MDL and MDX as parsed, with the models its reference nodes
/// name hung under them (`Model::attach_references`). A reference to a
/// model that is missing, does not read, or refers back to a model it hangs
/// under is left empty, with a warning.
pub fn parsed(resolver: &Resolver, name: &str) -> Result<Option<Model>, String> {
    parsed_under(resolver, name, &mut vec![name.to_ascii_lowercase()])
}

/// `parsed` for a model an area places: a room, a door or a placeable. A
/// model that does not read is logged and left out, as a missing one is,
/// so that one broken model does not stop its whole area loading.
fn parsed_or_left_out(resolver: &Resolver, name: &str) -> Option<Model> {
    parsed(resolver, name).unwrap_or_else(|why| {
        warn!("{why}; it is left out of the area");
        None
    })
}

/// `parsed`, inside the chain of models (`chain`, lower case) whose
/// reference nodes led to `name`.
fn parsed_under(
    resolver: &Resolver,
    name: &str,
    chain: &mut Vec<String>,
) -> Result<Option<Model>, String> {
    let Some(mut model) = parsed_alone(resolver, name)? else {
        return Ok(None);
    };
    let missing = model.attach_references(&mut |referenced| {
        let key = referenced.to_ascii_lowercase();
        if chain.contains(&key) {
            return None;
        }
        chain.push(key);
        let loaded = parsed_under(resolver, referenced, chain);
        chain.pop();
        loaded.unwrap_or_else(|why| {
            warn!("{name}: referenced model {referenced} is not read: {why}");
            None
        })
    });
    for referenced in missing {
        warn!("{name}: its reference node names {referenced}, which is not drawn");
    }
    Ok(Some(model))
}

/// A model's MDL and MDX as parsed, its reference nodes left empty.
fn parsed_alone(resolver: &Resolver, name: &str) -> Result<Option<Model>, String> {
    let Some(found) = resolver.find(name, ResType::MDL) else {
        return Ok(None);
    };
    let mdl = found.location.read().map_err(|error| error.to_string())?;
    let mdx = resolver
        .find(name, ResType::MDX)
        .map(|mdx| mdx.location.read().map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_else(Vec::new);
    Model::parse(&mdl, &mdx)
        .map(Some)
        .map_err(|error| format!("{name}.mdl: {error}"))
}

/// Makes the last keys of a model's clip its rest pose: each keyed node
/// moved by its position key (reone's rest + key), its alpha and
/// self-illumination those the clip ends on. A mine whose dome shows is
/// drawn so: its `detect` clip ends with its trigger's dome raised, lit
/// and opaque (`made`).
pub(crate) fn held(model: &mut Model, clip: &str) {
    use fmt_odyssey::mdl::{
        CONTROLLER_ALPHA as ALPHA, CONTROLLER_POSITION as POSITION,
        CONTROLLER_SELF_ILLUM as SELF_ILLUM,
    };
    fn last(node: &fmt_odyssey::mdl::Node, into: &mut BTreeMap<String, Vec<(u32, Vec<f32>)>>) {
        let rows = node.controllers.iter().filter_map(|controller| {
            let columns = usize::from(controller.columns);
            let stride = if controller.bezier {
                columns * 3
            } else {
                columns
            };
            let start = controller.times.len().checked_sub(1)? * stride;
            let row = controller.values.get(start..start + columns)?;
            [POSITION, SELF_ILLUM, ALPHA]
                .contains(&controller.kind)
                .then(|| (controller.kind, row.to_vec()))
        });
        into.entry(node.name.to_ascii_lowercase())
            .or_default()
            .extend(rows);
        for child in &node.children {
            last(child, into);
        }
    }
    fn pose(node: &mut fmt_odyssey::mdl::Node, keys: &BTreeMap<String, Vec<(u32, Vec<f32>)>>) {
        for (kind, row) in keys
            .get(&node.name.to_ascii_lowercase())
            .into_iter()
            .flatten()
        {
            if *kind == POSITION {
                if row.len() == 3 {
                    for (at, by) in node.position.iter_mut().zip(row) {
                        *at += by;
                    }
                }
                continue;
            }
            let rest = fmt_odyssey::mdl::Controller {
                kind: *kind,
                columns: row.len() as u8,
                bezier: false,
                times: vec![0.0],
                values: row.clone(),
            };
            match node.controllers.iter_mut().find(|c| c.kind == *kind) {
                Some(controller) => *controller = rest,
                None => node.controllers.push(rest),
            }
        }
        for child in &mut node.children {
            pose(child, keys);
        }
    }
    let Some(animation) = model
        .animations
        .iter()
        .find(|animation| animation.name.eq_ignore_ascii_case(clip))
    else {
        return;
    };
    let mut keys = BTreeMap::new();
    last(&animation.root, &mut keys);
    pose(&mut model.root, &keys);
}

/// Loads every texture and lightmap the placed models name.
pub(crate) fn textures<'m>(
    resolver: &Resolver,
    models: impl IntoIterator<Item = &'m Placed>,
) -> (BTreeMap<String, TextureIr>, Vec<String>) {
    let mut textures = BTreeMap::new();
    let mut missing = Vec::new();
    let mut load = |name: &String, textures: &mut BTreeMap<String, TextureIr>| {
        if textures.contains_key(name) || missing.contains(name) {
            return;
        }
        match texture(resolver, name) {
            Some(texture) => {
                textures.insert(name.clone(), texture);
            }
            None => missing.push(name.clone()),
        }
    };
    // A lightsaber's blades are saber meshes the model reader refuses; their
    // texture still gives a blade in a colour JA lacks its colour
    // (`BladeSpec::own`). A blade a mod ships is meshes its hilt grows from
    // nothing (`BladeSpec::meshes`).
    let mut names: Vec<String> = Vec::new();
    let mut objects_envmaps = Vec::new();
    for placed in models {
        objects_envmaps.extend(WornEnvMap::name(placed.envmap.as_ref()).cloned());
        names.extend(
            placed
                .ir
                .meshes
                .iter()
                .flat_map(|mesh| [mesh.texture.as_ref(), mesh.lightmap.as_ref()])
                .flatten()
                .cloned(),
        );
        names.extend(placed.ir.saber_textures().map(str::to_string));
        names.extend(
            placed
                .ir
                .folded
                .iter()
                .filter_map(|mesh| mesh.texture.clone()),
        );
    }
    for name in &names {
        load(name, &mut textures);
    }
    // Env maps the loaded textures' TXI names, and the objects'.
    let envmaps: Vec<String> = textures
        .values()
        .filter_map(|texture: &TextureIr| texture.facts.envmap.clone())
        .chain(objects_envmaps)
        .collect();
    for name in &envmaps {
        load(name, &mut textures);
    }
    // The bump maps water draws (`material::water`).
    let bumps: Vec<String> = textures
        .values()
        .filter_map(|texture: &TextureIr| {
            crate::material::water(Some(&texture.facts)).map(|(_, bump)| bump.to_string())
        })
        .collect();
    for name in &bumps {
        load(name, &mut textures);
    }
    (textures, missing)
}

/// What the scene draws where it names `name`, a texture in its `missing`:
/// per use, as reone and xoreos agree, except where noted.
/// - A lightmap: the mesh without one (a room's diffuse at full
///   brightness, an object lit at runtime).
/// - A diffuse whose mesh's lightmap loads: the lightmap alone (xoreos;
///   reone draws nothing, QUIRKS Q-0133).
/// - Any other diffuse: nothing, the mesh is not drawn.
/// - An env map: none.
/// - A body texture: the body model's own textures (reone, Q-0132).
pub fn stand_in(scene: &LoadedScene, name: &str) -> String {
    let mut uses = BTreeSet::new();
    if scene.missing_body_textures.iter().any(|body| body == name) {
        uses.insert("the body model's own textures");
    }
    for mesh in scene.models.iter().flat_map(|placed| &placed.ir.meshes) {
        if mesh.lightmap.as_deref() == Some(name) {
            uses.insert("no lightmap");
        }
        if mesh.texture.as_deref() == Some(name) {
            let lightmap = mesh.lightmap.as_ref();
            uses.insert(
                if lightmap.is_some_and(|lightmap| scene.textures.contains_key(lightmap)) {
                    "the lightmap alone"
                } else {
                    "nothing (the mesh is not drawn)"
                },
            );
        }
    }
    if scene
        .textures
        .values()
        .any(|texture| texture.facts.envmap.as_deref() == Some(name))
        || (scene.models.iter()).any(|placed| {
            WornEnvMap::name(placed.envmap.as_ref()).map(String::as_str) == Some(name)
        })
    {
        uses.insert("no env map");
    }
    if uses.is_empty() {
        uses.insert("nothing");
    }
    uses.into_iter().collect::<Vec<_>>().join("; ")
}

pub fn load_model(
    discovery: &Discovery,
    game: &str,
    name: &str,
    module: Option<&str>,
) -> Result<LoadedScene, String> {
    let game = game_of(game)?;
    let resolver = resolver(discovery, game, module)?;
    let ir = model(&resolver, name)?.ok_or(format!("{name}.mdl: not found"))?;
    let models = vec![Placed {
        ir,
        offset: [0.0; 3],
        facing: 0.0,
        is_static: false,
        attach: None,
        hook: None,
        hook_local: None,
        clip: None,
        envmap: None,
    }];
    let (textures, missing) = textures(&resolver, &models);
    Ok(LoadedScene {
        name: name.to_string(),
        models,
        textures,
        missing,
        missing_body_textures: Vec::new(),
        default_envmap: None,
        probes: Default::default(),
        entry: None,
        missing_models: Vec::new(),
        rooms: Vec::new(),
        own_fx: Vec::new(),
        instances: 0,
        creatures: 0,
        creature_refusals: Vec::new(),
        missing_clips: Vec::new(),
        creature_notes: Vec::new(),
        weapons: 0,
        ambient: DEFAULT_AMBIENT,
        fog: None,
        cameras: Vec::new(),
        map: None,
        cutscene_models: BTreeMap::new(),
        lit: false,
        blades: Vec::new(),
        shown: Vec::new(),
        effects: Effects::default(),
        doors: Vec::new(),
        instance_models: BTreeMap::new(),
        creature_views: Vec::new(),
        arms: BTreeMap::new(),
        ..LoadedScene::default()
    })
}

pub struct ResolverSource<'a>(pub &'a Resolver);

impl Source for ResolverSource<'_> {
    fn read(&self, resref: &str, restype: ResType) -> Option<Vec<u8>> {
        self.0.find(resref, restype)?.location.read().ok()
    }

    fn lent_script(&self, resref: &str) -> bool {
        vfs::lent::lent_script(self.0, resref)
    }

    fn exists(&self, resref: &str, restype: ResType) -> bool {
        self.0.find(resref, restype).is_some()
    }

    fn talk(&self) -> Option<std::sync::Arc<fmt_odyssey::tlk::Tlk>> {
        self.0.talk.get()
    }
}

/// Every room of the module's entry area at its layout position.
/// The archives of the modules that hold a creature's armour and its
/// headgear, when each is a module's own (`fmt_odyssey::module_of`).
pub type Owners<'a> = (Option<&'a Resolver>, Option<&'a Resolver>);

/// The scene's source, with one module's own item templates behind it. A
/// party member may wear a module's own armour far from that module (Echoes
/// of the Republic's robe from Sleheyron's palace). Its template is then
/// read from that module, as the original keeps the whole item with the
/// character. Without it the body was drawn bare.
pub(crate) struct OwnItems<'a> {
    pub(crate) scene: &'a ResolverSource<'a>,
    pub(crate) owner: Option<ResolverSource<'a>>,
}

impl OwnItems<'_> {
    fn owner_for(&self, restype: ResType) -> Option<&ResolverSource<'_>> {
        self.owner.as_ref().filter(|_| restype == ResType::UTI)
    }
}

impl Source for OwnItems<'_> {
    fn read(&self, resref: &str, restype: ResType) -> Option<Vec<u8>> {
        self.scene
            .read(resref, restype)
            .or_else(|| self.owner_for(restype)?.read(resref, restype))
    }

    fn exists(&self, resref: &str, restype: ResType) -> bool {
        self.scene.exists(resref, restype)
            || self
                .owner_for(restype)
                .is_some_and(|owner| owner.exists(resref, restype))
    }

    fn lent_script(&self, resref: &str) -> bool {
        self.scene.lent_script(resref)
    }

    fn talk(&self) -> Option<std::sync::Arc<fmt_odyssey::tlk::Tlk>> {
        self.scene.talk()
    }
}

pub fn load_module(discovery: &Discovery, game: &str, module: &str) -> Result<LoadedScene, String> {
    let game = game_of(game)?;
    let resolver = resolver(discovery, game, Some(module))?;
    let ir = ModuleIr::build(module, &ResolverSource(&resolver))
        .map_err(|refusal| format!("{module}: {refusal:?}"))?;
    let area = ir
        .areas
        .iter()
        .position(|area| area.resref == ir.entry_area)
        .or((!ir.areas.is_empty()).then_some(0))
        .ok_or(format!("{module}: no area"))?;
    module_scene(&resolver, game, &ir, area)
}

/// A room model's own clips: its `animloop…` clips in the model's order,
/// and its `scriptloopNN` clips by NN.
fn room_clips(ir: &ModelIr) -> (Vec<Arc<AnimationIr>>, BTreeMap<u8, Arc<AnimationIr>>) {
    let mut loops = Vec::new();
    let mut scripted = BTreeMap::new();
    for clip in &ir.animations {
        let name = clip.name.to_ascii_lowercase();
        if name.starts_with("animloop") {
            loops.push(Arc::new(clip.clone()));
        } else if let Some(n) = name
            .strip_prefix("scriptloop")
            .and_then(|n| n.parse::<u8>().ok())
        {
            scripted.insert(n, Arc::new(clip.clone()));
        }
    }
    (loops, scripted)
}

/// The lightmaps, in lower case, that the room meshes of `rooms` draw: the
/// meshes not hung under a reference node, as the renderer lights them as
/// rooms.
fn room_lightmaps(rooms: &[RoomView], models: &[Placed]) -> BTreeSet<String> {
    (rooms.iter())
        .filter_map(|room| models.get(room.model?))
        .flat_map(|placed| placed.ir.meshes.iter())
        .filter(|mesh| !mesh.referenced)
        .filter_map(|mesh| mesh.lightmap.as_deref())
        .map(str::to_ascii_lowercase)
        .collect()
}

/// The scene of `ir`'s area `area`: rooms, doors and placeables, creatures.
pub fn module_scene(
    resolver: &Resolver,
    game: Game,
    ir: &ModuleIr,
    area: usize,
) -> Result<LoadedScene, String> {
    let module = ir.name.as_str();
    let area = &ir.areas[area];
    let mut laps = crate::launch_times::Laps::new("kotor stage/scene");
    // Which bake of this area, if any, was made from the files the game
    // reads, before its lightmaps load (`detail_baked`).
    crate::lighting_bakes::note_played(game);
    choose_bakes(resolver, area, module);
    let used = used_folders(module);
    // Under Enhanced Lightmaps an area with no accepted bake is drawn as
    // under Enhanced (`fitted_lights::baked_area`).
    let area_baked = crate::fitted_lights::baked_area(takes_bake(&used, &area.resref));
    let dark = dark_unlit(&used, &area.resref, module);
    let walkable = walkable_surfaces(resolver);
    let mut models = Vec::new();
    let mut missing_models = Vec::new();
    let mut rooms = Vec::new();
    let mut own_fx = Vec::new();
    let mut grass = area
        .grass
        .as_ref()
        .map(|grass| SceneGrass::of(resolver, grass, area.weather.wind));
    // The surfaces grass grows on; none when there is no grass to draw.
    let grassy = match &grass {
        Some(drawn) if !drawn.missing_texture => Surfaces::read(&ResolverSource(resolver))
            .map_or_else(Vec::new, |surfaces| surfaces.grass()),
        _ => Vec::new(),
    };
    let without_vis = area.missing.contains(&Missing::Visibility);
    for room in &area.rooms {
        let index = match parsed_or_left_out(resolver, &room.model) {
            Some(parsed) => {
                if let (Some(placing), Some(drawn)) = (&area.grass, grass.as_mut())
                    && !grassy.is_empty()
                {
                    let faces = asset_odyssey::grass::faces(&parsed, &grassy);
                    let tufts = asset_odyssey::grass::tufts(&faces, placing, room.position);
                    if !tufts.is_empty() {
                        drawn.patches.push(GrassPatch {
                            room: rooms.len(),
                            tufts,
                            lightmap: faces.lightmap,
                        });
                    }
                }
                let ir = ModelIr::from_model(&parsed);
                let fx = ModelFx::read(&parsed);
                let (loops, scripted) = room_clips(&ir);
                if !loops.is_empty() || !scripted.is_empty() || !fx.emitters.is_empty() {
                    own_fx.push(OwnFx {
                        model: models.len(),
                        plays: Plays::Room {
                            loops,
                            scripted,
                            room: sim::room_key(&room.model),
                        },
                        chunks: Arc::new(fx.chunks(resolver)),
                        fx: Arc::new(fx),
                    });
                }
                models.push(Placed {
                    ir,
                    offset: room.position,
                    facing: 0.0,
                    is_static: true,
                    attach: None,
                    hook: None,
                    hook_local: None,
                    clip: None,
                    envmap: None,
                });
                Some(models.len() - 1)
            }
            None => {
                missing_models.push(room.model.clone());
                None
            }
        };
        let (floor, floor_error) = match floor(resolver, &room.model, &walkable) {
            Ok(floor) => (floor, None),
            Err(why) => (Vec::new(), Some(why)),
        };
        rooms.push(RoomView {
            name: room.model.clone(),
            model: index,
            visible: room.visible.clone(),
            sees_all: without_vis,
            floor,
            floor_error,
        });
    }
    laps.lap("rooms");
    // Doors and placeables at their GIT position and bearing (as xoreos places them): doors in their rest pose, placeables posed by
    // their state's clip with their emitters (`own_fx`).
    let mut creatures = CreatureLoad {
        default_envmap: area.default_envmap.clone(),
        ..CreatureLoad::default()
    };
    let loaded = &mut creatures.models;
    let mut instances = 0;
    let mut doors = Vec::new();
    let mut instance_models = BTreeMap::new();
    // Each placeable model's own clips and emitters, read once.
    let mut fx_of: BTreeMap<String, Option<OwnFx>> = BTreeMap::new();
    // Each door model's clips' alpha (a force field fades as it opens).
    let mut door_fx: BTreeMap<String, Arc<ModelFx>> = BTreeMap::new();
    // Which meshes of each placeable model the bake may light, so that they
    // take its light in place of the probes' (`placeable_light::bends`).
    let mut lit_of: BTreeMap<String, Arc<Vec<bool>>> = BTreeMap::new();
    // The model of each placeable with a mesh the bake may light, by its
    // object: its number in the scene, its name and which meshes those are.
    let mut lit: LitPlaceables = BTreeMap::new();
    for (object, instance) in area.instances.iter().enumerate() {
        let Some(name) = &instance.model else {
            continue;
        };
        if !loaded.contains_key(name) {
            let parsed = parsed_or_left_out(resolver, name);
            let ir = parsed.as_ref().map(ModelIr::from_model);
            if instance.kind == Kind::Placeable
                && let (Some(parsed), Some(ir)) = (&parsed, &ir)
            {
                let own = placeable_fx(
                    (&mut *loaded, resolver, game),
                    parsed,
                    ir,
                    (0, sim::look::DEFAULT),
                );
                fx_of.insert(name.clone(), own);
                if area_baked && crate::placeable_light::baked_light() {
                    lit_of.insert(
                        name.clone(),
                        Arc::new(crate::placeable_light::lit_meshes(ir)),
                    );
                }
            }
            if instance.kind == Kind::Door
                && let Some(parsed) = &parsed
            {
                let fx = fx_through_chain(resolver, parsed);
                door_fx.insert(name.clone(), Arc::new(fx));
            }
            loaded.insert(name.clone(), ir);
        }
        if instance.kind == Kind::Placeable
            && let Some(Some(own)) = fx_of.get(name)
        {
            let placed = sim::look::placed(instance.animation_state);
            own_fx.push(own.placed_as(models.len(), (object as u32, placed)));
        }
        if instance.kind == Kind::Door
            && let Some(ir) = loaded[name].clone()
        {
            let mut clips = |names: [&str; 2]| -> Result<[Option<Arc<AnimationIr>>; 2], String> {
                Ok([
                    clip(loaded, resolver, game, &ir, names[0])?,
                    clip(loaded, resolver, game, &ir, names[1])?,
                ])
            };
            doors.push(DoorView {
                object: object as u32,
                model: models.len(),
                opening: clips(["opening1", "opening2"])?,
                closing: clips(["closing1", "closing2"])?,
                busted: match instance.busted {
                    true => clip(loaded, resolver, game, &ir, "busted")?,
                    false => None,
                },
                fx: door_fx.get(name).cloned(),
            });
        }
        match &loaded[name] {
            Some(ir) => {
                if instance.kind == Kind::Placeable
                    && let Some(meshes) = lit_of
                        .get(name)
                        .filter(|meshes| meshes.iter().any(|lit| *lit))
                {
                    lit.insert(object as u32, (models.len(), name.clone(), meshes.clone()));
                }
                instance_models.insert(object as u32, models.len());
                models.push(Placed {
                    ir: ir.clone(),
                    offset: instance.position,
                    facing: instance.facing,
                    // A template's `Static` says how it is used (not
                    // selectable), not how it is lit: only rooms take the
                    // lightmap alone, so a static placeable with none (a
                    // body laid out by the module) keeps the area's light.
                    is_static: false,
                    attach: None,
                    hook: None,
                    hook_local: None,
                    clip: None,
                    envmap: None,
                });
                instances += 1;
            }
            None => missing_models.push(name.clone()),
        }
    }
    laps.lap("doors and placeables");
    // Creatures in their rest pose, at their GIT position and bearing.
    let mut creature_views = Vec::new();
    for (object, instance) in area
        .instances
        .iter()
        .enumerate()
        .filter(|(_, i)| i.kind == Kind::Creature)
    {
        let placed = creatures.place(
            (resolver, game),
            (object as u32, &instance.template),
            (instance.position, instance.facing),
            &mut models,
        )?;
        own_fx.extend(placed.as_ref().and_then(|view| view.fx.clone()));
        creature_views.extend(placed);
    }
    missing_models.append(&mut creatures.missing_models);
    laps.lap("creatures");
    let (mut textures, mut missing) = textures(resolver, &models);
    for room in &own_fx {
        crate::model_fx::add_textures(resolver, (&room.fx, &room.chunks), &mut textures);
    }
    for name in grass.iter().flat_map(SceneGrass::textures) {
        if !textures.contains_key(name)
            && let Some(texture) = texture(resolver, name)
        {
            textures.insert(name.clone(), texture);
        }
    }
    let stored_lightmaps = baked_lightmaps(&used, &room_lightmaps(&rooms, &models), &mut textures);
    laps.lap("textures");
    let stored = Stored {
        resolver,
        lightmaps: &stored_lightmaps,
    };
    let (baked, sun) = baked_probes((stored, &used), &rooms, area_baked);
    let mut probes = crate::light_probes::bake(&models, &rooms, &textures, baked);
    probes.sun = sun;
    probes.placeables = baked_placeables((stored, &used), &area.resref, &lit, area_baked);
    laps.lap("light probes");
    missing.extend(creatures.missing_body_textures.iter().cloned());
    Ok(LoadedScene {
        probes: Arc::new(probes),
        baked: area_baked,
        dark_unlit: dark,
        stored_lightmaps,
        default_envmap: creatures.default_envmap,
        missing_body_textures: creatures.missing_body_textures,
        instances,
        creatures: creatures.placed,
        creature_refusals: creatures.refusals,
        missing_clips: creatures.missing_clips,
        creature_notes: creatures.notes,
        weapons: creatures.weapons,
        // As reone has it: the ARE's colour when non-zero.
        ambient: match area.dyn_ambient {
            Some(packed) if packed > 0 => unpack_colour(packed),
            _ => DEFAULT_AMBIENT,
        },
        fog: area.fog.map(|fog| Fog {
            color: unpack_colour(fog.color),
            near: fog.near,
            far: fog.far,
        }),
        cameras: area.cameras.clone(),
        pull_backs: pull_backs(resolver),
        map: area.map,
        cutscene_models: BTreeMap::new(),
        lit: true,
        blades: Vec::new(),
        shown: Vec::new(),
        effects: Effects::default(),
        doors,
        instance_models,
        creature_views,
        arms: BTreeMap::new(),
        sound: None,
        name: module.to_string(),
        models,
        textures,
        images: Default::default(),
        missing,
        entry: Some((ir.entry_position, ir.entry_direction)),
        missing_models,
        rooms,
        own_fx,
        grass,
        weather: SceneWeather {
            area: area.weather,
            resref: area.resref.clone(),
            dry_rooms: area.rooms.iter().map(|room| room.disable_weather).collect(),
            model: weather_model(resolver, &area.weather).map(Arc::new),
            lightning: crate::weather::lightning(resolver, game, &area.weather).map(Arc::new),
        },
    })
}

/// A body model wearing `texture` (`appearance.2da`'s, when the install
/// has it): as reone retextures, every body mesh, textured or not (the
/// TSL soldier bodies' torso and arms name none of their own).
pub(crate) fn retexture(body: &mut ModelIr, texture: Option<&str>) {
    if let Some(texture) = texture {
        for mesh in &mut body.meshes {
            mesh.texture = Some(texture.to_string());
        }
    }
}

/// A head model wearing `texture` (`creature::CreatureLook::head_texture`)
/// on every mesh that has a texture, as the executables replace a head's
/// texture for its alignment.
pub(crate) fn retexture_head(head: &mut ModelIr, texture: Option<&str>) {
    if let Some(texture) = texture {
        for mesh in head.meshes.iter_mut().filter(|mesh| mesh.texture.is_some()) {
            mesh.texture = Some(texture.to_string());
        }
    }
}

#[cfg(feature = "probes")]
pub mod bake_probes;
#[cfg(feature = "probes")]
pub mod kept_probes;
#[cfg(feature = "probes")]
pub mod undrawn_probes;

/// How a creature of alignment `good_evil` (none known: neutral) is dressed
/// in `game` (`alignment_look`).
pub(crate) fn aligned_of(game: Game, good_evil: Option<u8>) -> creature::Aligned {
    crate::alignment_look::in_game(game, good_evil.unwrap_or(sim::combat::sheet::NEUTRAL))
}

/// What the party table keeps of a companion made from a template that no
/// module of the session ships (`sim::Level::template`). KOTOR keeps the
/// whole creature in its party table, so a module that lacks the template
/// still draws it. Dark side Bastila is one: her template is the Temple
/// Summit's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Kept {
    /// Its `appearance.2da` row.
    pub appearance: u32,
    /// The items its template equips on its body and its head (UTI
    /// resrefs).
    pub armour: Option<String>,
    pub headgear: Option<String>,
    /// The items its template holds in its right and left hands.
    pub weapons: [Option<String>; 2],
    /// Its alignment as the party table keeps it.
    pub good_evil: Option<u8>,
}

impl Kept {
    /// What `facts` (a creature's) keep of its look; `None` for one that
    /// names no `appearance.2da` row.
    pub fn of(facts: &sim::module::ObjectData) -> Option<Kept> {
        use sim::inventory::slots;
        let worn = |slot| {
            facts
                .inventory
                .iter()
                .find(|held| held.slot == Some(slot))
                .map(|held| sim::inventory::resref_of(&held.template).to_ascii_lowercase())
        };
        Some(Kept {
            appearance: u32::from(facts.body?.appearance),
            armour: worn(slots::BODY),
            headgear: worn(slots::HEAD),
            weapons: [worn(slots::RIGHT_WEAPON), worn(slots::LEFT_WEAPON)],
            good_evil: facts.sheet.map(|sheet| sheet.good_evil),
        })
    }
}

/// What placing creatures reads once and reports: every model read (by
/// name; `None` where the install lacks it), each template's look and each
/// humanoid body's scripted clips, and what was placed, refused or missing.
#[derive(Default)]
pub(crate) struct CreatureLoad {
    pub models: BTreeMap<String, Option<ModelIr>>,
    looks: BTreeMap<String, Result<creature::CreatureLook, creature::CreatureRefusal>>,
    /// The tables the looks read, read once.
    tables: asset_odyssey::tables::Tables,
    /// Each body's named clips (by body, whether it is a creature type,
    /// and the creature clip's name), looked up once.
    named: BTreeMap<(String, bool, &'static str), Option<Arc<AnimationIr>>>,
    acted: BTreeMap<String, Vec<(u16, Arc<AnimationIr>)>>,
    /// Each body's emitters (`None`: it has none), read once.
    emitters: BTreeMap<String, Option<OwnFx>>,
    /// Death lines' clips by sound, read once (`None`: not in the install).
    cries: BTreeMap<String, Option<audio::Clip>>,
    /// Creatures placed, and the weapons placed in their hands.
    pub placed: usize,
    pub weapons: usize,
    /// Templates whose look did not resolve, bodies with no idle whose
    /// supermodel chain is broken, models the install lacks.
    pub refusals: Vec<String>,
    pub missing_clips: Vec<String>,
    pub missing_models: Vec<String>,
    /// Body textures `appearance.2da` names that the install lacks: those
    /// bodies keep their model's own textures.
    pub missing_body_textures: Vec<String>,
    /// One line per creature: models and hook positions (for checking).
    pub notes: Vec<String>,
    /// The area's `DefaultEnvMap` (`creature::EnvMap::AreaDefault`).
    pub default_envmap: Option<String>,
}

impl CreatureLoad {
    /// `template`'s look, read the first time.
    fn look(
        &mut self,
        resolver: &Resolver,
        template: &str,
    ) -> &Result<creature::CreatureLook, creature::CreatureRefusal> {
        let tables = &mut self.tables;
        self.looks
            .entry(template.to_ascii_lowercase())
            .or_insert_with(|| creature::resolve_with(&ResolverSource(resolver), template, tables))
    }

    /// How many refusals and missing models are noted so far: what
    /// `why_not_drawn` reads past.
    pub fn noted(&self) -> (usize, usize) {
        (self.refusals.len(), self.missing_models.len())
    }

    /// Why a creature placed since `noted` gave `since` was not drawn: its
    /// look's refusal (an `appearance.2da` row the install lacks, or one
    /// naming no body), or the body model the install lacks.
    pub fn why_not_drawn(&self, (refusals, models): (usize, usize)) -> String {
        let refused = self.refusals.get(refusals..).and_then(<[String]>::last);
        let missing = self.missing_models.get(models..).and_then(<[String]>::last);
        match (refused, missing) {
            (Some(refusal), _) => format!("refused, {refusal}"),
            (None, Some(model)) => format!("no model {model}"),
            (None, None) => "no body".to_string(),
        }
    }

    /// Whether `resolver` has `template` (a module's own creatures are in
    /// its archives only).
    pub fn has(&mut self, resolver: &Resolver, template: &str) -> bool {
        !matches!(
            self.look(resolver, template),
            Err(creature::CreatureRefusal::NoTemplate(_))
        )
    }

    /// Creature `object` of `template`, standing at `position` with the
    /// bearing `facing`, in its rest pose: its body (with its body texture),
    /// its head at the body's `headhook`, and its weapons in its hands,
    /// pushed onto `models`. How the views animate it, or `None` when its
    /// look does not resolve or its body is missing.
    pub fn place(
        &mut self,
        (resolver, game): (&Resolver, Game),
        (object, template): (u32, &str),
        (position, facing): ([f32; 3], f32),
        models: &mut Vec<Placed>,
    ) -> Result<Option<CreatureView>, String> {
        let look = match self.look(resolver, template).clone() {
            Ok(look) => look,
            Err(refusal) => {
                self.refusals.push(format!("{template}: {refusal:?}"));
                return Ok(None);
            }
        };
        let aligned = aligned_of(game, look.good_evil);
        self.place_look(
            (resolver, game),
            (object, template, look, aligned),
            (position, facing),
            models,
        )
    }

    /// A creature of `appearance.2da` row `appearance` wearing `armour` (a
    /// UTI) or nothing on its body, put on or taken off as `dressing` says,
    /// and `headgear` (a UTI) or nothing on its head, with the head of
    /// `heads.2da` row `head` where one is given (else the row's own),
    /// dressed for `aligned`, as `place` places one of a template. `owners`
    /// are the archives of the modules that hold the armour and the
    /// headgear when they are a module's own (`worn_model`).
    pub fn place_appearance(
        &mut self,
        (resolver, game): (&Resolver, Game),
        (appearance, (armour, dressing), head, headgear): Outfit<'_>,
        (armour_owner, owner): Owners<'_>,
        aligned: creature::Aligned,
        (position, facing): ([f32; 3], f32),
        models: &mut Vec<Placed>,
    ) -> Result<Option<CreatureView>, String> {
        let name = format!("appearance {appearance}");
        let source = ResolverSource(resolver);
        let dressed = OwnItems {
            scene: &source,
            owner: armour_owner.map(ResolverSource),
        };
        let mut look = creature::resolve_dressed(
            &dressed,
            appearance,
            armour.map(str::to_string),
            dressing,
            &mut self.tables,
        )
        .map_err(|refusal| format!("{name}: {refusal:?}"))?;
        if let Some(head) = head {
            look.head = (self.tables.get(&source, "heads"))
                .and_then(|heads| heads.resref(usize::from(head), "head"));
            look.head_row = Some(usize::from(head));
        }
        look.headgear =
            headgear.and_then(|item| worn_model(owner, &source, &mut self.tables, item));
        self.place_look(
            (resolver, game),
            (0, &name, look, aligned),
            (position, facing),
            models,
        )
    }

    /// A creature whose template no module of the session ships, placed as
    /// `place` places one of a template. Its look is built from what the
    /// party table keeps of it (`Kept`). Its sound set is not kept, so it
    /// has none.
    pub fn place_kept(
        &mut self,
        (resolver, game): (&Resolver, Game),
        (object, template, kept): (u32, &str, &Kept),
        (position, facing): ([f32; 3], f32),
        models: &mut Vec<Placed>,
    ) -> Result<Option<CreatureView>, String> {
        let source = ResolverSource(resolver);
        let tables = &mut self.tables;
        let mut look = match creature::resolve_appearance(
            &source,
            kept.appearance,
            kept.armour.clone(),
            tables,
        ) {
            Ok(look) => look,
            Err(refusal) => {
                self.refusals.push(format!("{template}: {refusal:?}"));
                return Ok(None);
            }
        };
        let mut model = |item: &Option<String>| {
            item.as_deref()
                .and_then(|item| creature::item_model_with(&source, item, tables))
        };
        look.headgear = model(&kept.headgear);
        look.right_weapon = model(&kept.weapons[0]);
        look.left_weapon = model(&kept.weapons[1]);
        let aligned = aligned_of(game, kept.good_evil);
        self.place_look(
            (resolver, game),
            (object, template, look, aligned),
            (position, facing),
            models,
        )
    }

    /// `look` (`template`'s, as the notes name it), dressed for `aligned`
    /// (`creature::align`), placed as `place` says.
    fn place_look(
        &mut self,
        (resolver, game): (&Resolver, Game),
        (object, template, mut look, aligned): (
            u32,
            &str,
            creature::CreatureLook,
            creature::Aligned,
        ),
        (position, facing): ([f32; 3], f32),
        models: &mut Vec<Placed>,
    ) -> Result<Option<CreatureView>, String> {
        creature::align(
            &ResolverSource(resolver),
            &mut look,
            aligned,
            &mut self.tables,
        );
        let loaded = &mut self.models;
        let Some(mut body) = cached(loaded, resolver, &look.body)? else {
            self.missing_models.push(look.body.clone());
            return Ok(None);
        };
        retexture(&mut body, look.body_texture.as_deref());
        if let Some(texture) = &look.missing_body_texture
            && !self.missing_body_textures.contains(texture)
        {
            self.missing_body_textures.push(texture.clone());
        }
        let hook_node = body.node_index(hooks::HEAD_HOOK);
        let hook = hook_node.map(|node| body.nodes[node].world);
        let world_of = |name: &str| {
            body.node(name)
                .map(|node| [node.world[0][3], node.world[1][3], node.world[2][3]])
        };
        self.notes.push(format!(
            "{template} at {position:?}: body {} (super {:?}), head {:?}, headhook {:?}, head node {:?}, type {}",
            look.body,
            body.supermodel,
            look.head,
            world_of(hooks::HEAD_HOOK),
            world_of("head"),
            look.model_type
        ));
        let creature_type = creature::creature_model(&look.model_type);
        let found = &mut self.named;
        let mut named = |loaded: &mut BTreeMap<_, _>, creature: &'static str, other| {
            let key = (look.body.clone(), creature_type, creature);
            if let Some(clip) = found.get(&key) {
                return Ok(clip.clone());
            }
            let clip = creature_clip(
                loaded,
                resolver,
                game,
                &body,
                creature_type,
                creature,
                other,
            )?;
            found.insert(key, clip.clone());
            Ok::<_, String>(clip)
        };
        let idle = named(loaded, "cpause1", "pause1")?;
        if idle.is_none() {
            // A body whose whole chain has no idle stands at rest, as in
            // the game (the turrets); only a broken chain is a fault.
            match broken_chain(loaded, resolver, &body)? {
                Some(missing) => self.missing_clips.push(format!(
                    "{}: idle (supermodel {missing} missing)",
                    look.body
                )),
                None => self
                    .notes
                    .push(format!("{}: no idle, stands at rest", look.body)),
            }
        }
        let humanoid = asset_ja::rig::is_humanoid(&body);
        let lit = !body.lights.is_empty();
        // Each body's scripted clips, looked up once, a creature model
        // type's by its creature names first.
        let acted = match self.acted.get(&look.body) {
            Some(acted) => acted.clone(),
            None => {
                let acted = scripted_clips(loaded, resolver, (game, creature_type), &body)?;
                self.acted.insert(look.body.clone(), acted.clone());
                acted
            }
        };
        let source = ResolverSource(resolver);
        let strides = creature::strides(&source, look.appearance, &mut self.tables)
            .unwrap_or(crate::play::STRIDES);
        let ground_tilt = creature::ground_tilt(&source, look.appearance, &mut self.tables);
        let clips = if humanoid {
            // Its own clips out of a fight (`gait`).
            CreatureClips {
                pause: idle.clone(),
                walk: named(loaded, "cwalk", "walk")?,
                run: named(loaded, "crun", "run")?,
                acted,
                strides,
                creature: creature_type,
                ..CreatureClips::default()
            }
        } else {
            CreatureClips {
                pause: idle.clone(),
                walk: named(loaded, "cwalk", "walk")?,
                run: named(loaded, "crun", "run")?,
                die: named(loaded, "cdie", "die")?,
                dead: named(loaded, "cdead", "dead")?,
                getup: named(loaded, "cgetupdead", "getupdead")?,
                acted,
                strides,
                creature: creature_type,
            }
        };
        let body_index = models.len();
        // Weapons at their hand nodes (as reone hangs them: right weapon on
        // `rhand`, left on `lhand`), static in the hand.
        let weapons: Vec<(usize, String, usize, Matrix)> = [
            (look.right_weapon.as_ref(), hooks::RIGHT_HAND),
            (look.left_weapon.as_ref(), hooks::LEFT_HAND),
        ]
        .into_iter()
        .enumerate()
        .filter_map(|(slot, (model, hand))| {
            let node = body.node_index(hand)?;
            Some((slot, model?.clone(), node, body.nodes[node].world))
        })
        .collect();
        // Models face +Y; a creature's GIT direction turns it by
        // `-atan2(x, y)` (as xoreos and reone read it): its angle minus 90°.
        let turn = facing - std::f32::consts::FRAC_PI_2;
        // The body, and what hangs from its nodes, at the creature's place.
        let at = |ir: ModelIr| Placed {
            facing: turn,
            ..Placed::at(ir, position)
        };
        // Its body and head take its env map, its weapons do not.
        let envmap = Some(WornEnvMap(
            look.envmap.resolve(self.default_envmap.as_deref()),
        ));
        models.push(Placed {
            clip: idle.clone(),
            envmap: envmap.clone(),
            ..at(body)
        });
        self.placed += 1;
        if let (Some(head), Some(hook), Some(hook_node)) = (&look.head, hook, hook_node) {
            match cached(loaded, resolver, head)? {
                Some(mut ir) => {
                    retexture_head(&mut ir, look.head_texture.as_deref());
                    models.push(Placed {
                        attach: Some(hook),
                        hook: Some((body_index, hook_node)),
                        clip: idle.clone(),
                        envmap,
                        ..at(ir)
                    });
                    let head = models.len() - 1;
                    if let Some(missing) =
                        hang_headgear(loaded, resolver, models, head, look.headgear.as_deref())?
                    {
                        self.missing_models.push(missing);
                    }
                }
                None => self.missing_models.push(head.clone()),
            }
        }
        let mut held = [None; 2];
        for (slot, weapon, node, world) in weapons {
            match cached(loaded, resolver, &weapon)? {
                Some(ir) => {
                    held[slot] = Some(models.len());
                    models.push(Placed {
                        attach: Some(world),
                        hook: Some((body_index, node)),
                        ..at(ir)
                    });
                    self.weapons += 1;
                }
                None => self.missing_models.push(weapon),
            }
        }
        let cry = look.death_sound.as_ref().and_then(|name| {
            let clip = self
                .cries
                .entry(name.clone())
                .or_insert_with(|| cry_clip(resolver, name))
                .clone()?;
            Some((name.clone(), clip))
        });
        let fx = self.body_fx(resolver, &look.body, lit).map(|fx| OwnFx {
            model: body_index,
            ..fx
        });
        Ok(Some(CreatureView {
            object,
            body: body_index,
            humanoid,
            clips,
            weapons: held,
            cry,
            sound_set: look.sound_set,
            fx,
            ground_tilt,
            aligned: look.head_row.map(|_| aligned),
            hologram: look.hologram,
        }))
    }

    /// Body `name`'s emitters and lights (`Plays::Body`; `lit`: it has
    /// lights), read the first time; `None` for a body with neither, or
    /// whose model does not read.
    fn body_fx(&mut self, resolver: &Resolver, name: &str, lit: bool) -> Option<OwnFx> {
        if let Some(fx) = self.emitters.get(name) {
            return fx.clone();
        }
        let fx = match parsed(resolver, name) {
            Ok(parsed) => parsed.map(|parsed| fx_through_chain(resolver, &parsed)),
            Err(why) => {
                warn!("{name}: its emitters are not read: {why}");
                None
            }
        };
        let fx = fx
            .filter(|fx| lit || !fx.emitters.is_empty())
            .map(|fx| OwnFx {
                model: 0,
                plays: Plays::Body,
                chunks: Arc::new(fx.chunks(resolver)),
                fx: Arc::new(fx),
            });
        self.emitters.insert(name.to_string(), fx.clone());
        fx
    }
}

/// A death line's clip: a streamed sound, or a WAV in the archives.
fn cry_clip(resolver: &Resolver, name: &str) -> Option<audio::Clip> {
    let bytes = match resolver.voice(name) {
        Some(location) => location.read().ok()?,
        None => resolver.find(name, ResType::WAV)?.location.read().ok()?,
    };
    audio::Clip::kotor(&bytes).ok()
}

/// An ARE colour, packed with red in the low byte (as reone
/// reads it).
fn unpack_colour(packed: u32) -> [f32; 3] {
    [0, 8, 16].map(|shift| ((packed >> shift) & 0xff) as f32 / 255.0)
}

/// The viewers' field of view (degrees): `camerastyle.2da`'s first row's
/// view angle.
pub const DEFAULT_VIEW_ANGLE: f32 = 55.0;

/// `dialoganimations.2da`'s `CU_PB_Range`, row by row (0 where blank).
pub fn pull_backs(resolver: &Resolver) -> Vec<f32> {
    resolver
        .two_da("dialoganimations")
        .map_or_else(Vec::new, |table| {
            (0..table.cells.len())
                .map(|row| table.float(row, "CU_PB_Range").unwrap_or(0.0))
                .collect()
        })
}

/// Rows of `surfacemat.2da` whose `Walk` is set (`collision_odyssey::Surfaces`).
fn walkable_surfaces(resolver: &Resolver) -> Vec<bool> {
    resolver
        .two_da("surfacemat")
        .map_or_else(Vec::new, |table| {
            collision_odyssey::Surfaces::from_2da(&table)
                .rows
                .iter()
                .map(|row| row.walk)
                .collect()
        })
}

/// The walkable faces of a room's walkmesh. Room walkmeshes are stored in
/// world space (reone adds them to the scene untransformed).
fn floor(resolver: &Resolver, room: &str, walkable: &[bool]) -> Result<Vec<[[f32; 3]; 3]>, String> {
    let Some(found) = resolver.find(room, ResType::WOK) else {
        return Ok(Vec::new());
    };
    let walkmesh = found
        .location
        .read()
        .map_err(|error| error.to_string())
        .and_then(|bytes| Walkmesh::parse(&bytes).map_err(|error| error.to_string()))
        .map_err(|why| format!("{room}.wok: {why}"))?;
    if walkable.is_empty() && !walkmesh.faces.is_empty() {
        return Err(format!(
            "{room}.wok: no walkable materials (surfacemat.2da)"
        ));
    }
    Ok(walkmesh
        .faces
        .iter()
        .filter(|face| {
            walkable
                .get(face.material as usize)
                .copied()
                .unwrap_or(false)
        })
        .filter_map(|face| {
            let corner = |i: usize| walkmesh.vertices.get(face.vertices[i] as usize).copied();
            Some([corner(0)?, corner(1)?, corner(2)?])
        })
        .collect())
}

/// The model `item` (an item template) is drawn with: read from `source`,
/// else, for a module's own template (`sim::inventory::own_template`), from
/// `owner`, the archives of the module that holds it. A party member may
/// wear such an item far from that module (Davik's Visor, from his estate
/// on Taris), where `source` lacks its template.
pub fn worn_model(
    owner: Option<&Resolver>,
    source: &ResolverSource,
    tables: &mut asset_odyssey::tables::Tables,
    item: &str,
) -> Option<String> {
    creature::item_model_with(source, item, tables)
        .or_else(|| creature::item_model(&ResolverSource(owner?), item))
}

/// A model through a per-load cache.
/// `headgear` (a model) hung on the head `models[head]` at its
/// `gogglehook`, as reone hangs a mask: it follows that node, as a weapon
/// follows its hand. Nothing for a head without the hook; the model's
/// name when the install lacks it.
pub fn hang_headgear(
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    models: &mut Vec<Placed>,
    head: usize,
    headgear: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(name) = headgear else {
        return Ok(None);
    };
    let Some(node) = models[head].ir.node_index(hooks::HEADGEAR_HOOK) else {
        return Ok(None);
    };
    let Some(ir) = cached(cache, resolver, name)? else {
        return Ok(Some(name.to_string()));
    };
    let on = &models[head];
    let placed = Placed {
        hook: Some((head, node)),
        facing: on.facing,
        ..Placed::at(ir, on.offset)
    };
    models.push(placed);
    Ok(None)
}

pub fn cached(
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    name: &str,
) -> Result<Option<ModelIr>, String> {
    Ok(read_into(cache, resolver, name)?.cloned())
}

/// A model in the per-load cache, read the first time, borrowed (a clip's
/// lookup through a supermodel chain copies no model).
fn read_into<'c>(
    cache: &'c mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    name: &str,
) -> Result<Option<&'c ModelIr>, String> {
    if !cache.contains_key(name) {
        cache.insert(name.to_string(), model(resolver, name)?);
    }
    Ok(cache[name].as_ref())
}

/// A clip by name from a model or its supermodel chain (as reone
/// finds it: the model's own first, then its supermodel's).
/// The chain stops at `null`, an empty name, a missing model, or a model
/// already seen.
pub fn clip(
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    game: Game,
    model: &ModelIr,
    name: &str,
) -> Result<Option<Arc<AnimationIr>>, String> {
    // One clip, shared by every body whose chain it is found in.
    let find = |model: &ModelIr| {
        let key = (
            format!("{game:?} {}", model.name.to_ascii_lowercase()),
            name.to_string(),
        );
        let mut shared = SHARED_CLIPS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(found) = shared.get(&key) {
            return Some(found.clone());
        }
        let found = Arc::new(model.animations.iter().find(|a| a.name == name)?.clone());
        shared.insert(key, found.clone());
        Some(found)
    };
    if let Some(found) = find(model) {
        return Ok(Some(found));
    }
    let talking = name.starts_with("tlk");
    let mut seen = vec![model.name.to_ascii_lowercase()];
    let mut next = next_supermodel(model, &mut seen);
    while let Some(super_name) = next {
        let Some(parent) = read_into(cache, resolver, &super_name)? else {
            break;
        };
        let found = find(parent).filter(|found| !talking || shares_rest_pose(model, parent, found));
        if found.is_some() {
            return Ok(found);
        }
        next = next_supermodel(parent, &mut seen);
    }
    Ok(None)
}

const REST_TURN_TOLERANCE_DEGREES: f32 = 20.0;

/// A clip keys absolute turns, so it only suits a body that rests as its
/// supermodel does. A Rakata does not: its eye nodes rest at about 90 degrees,
/// and the human `tlknorm` of `s_male02` turns them 86 to 178 degrees away.
fn shares_rest_pose(body: &ModelIr, parent: &ModelIr, clip: &AnimationIr) -> bool {
    clip.tracks
        .iter()
        .filter(|track| !track.orientations.is_empty())
        .all(|track| {
            let (Some(own), Some(shared)) = (body.node(&track.node), parent.node(&track.node))
            else {
                return true;
            };
            let turn = bevy::math::Quat::from_array(own.rotation)
                .normalize()
                .angle_between(bevy::math::Quat::from_array(shared.rotation).normalize());
            turn.to_degrees() <= REST_TURN_TOLERANCE_DEGREES
        })
}

/// The clips `clip` found, by the model whose animation each is and its
/// name: a body's chain shares its supermodel's.
static SHARED_CLIPS: std::sync::Mutex<BTreeMap<(String, String), Arc<AnimationIr>>> =
    std::sync::Mutex::new(BTreeMap::new());

/// A body's clips for what scripts and the engine play on it:
/// `acting::clips` (with `creature`, a creature model's own `c…` name
/// first, `creature_clip`), `animations.2da`'s rows by number
/// (`creature::row_clips`: a round's swings and answers among them,
/// `acting::stance_of`), the first of each constant's names the body's
/// chain has.
pub fn scripted_clips(
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    (game, creature): (Game, bool),
    body: &ModelIr,
) -> Result<Vec<(u16, Arc<AnimationIr>)>, String> {
    let mut acted = Vec::new();
    for (animation, name) in crate::acting::clips() {
        let own = format!("c{name}");
        if let Some(found) = creature_clip(cache, resolver, game, body, creature, &own, name)? {
            acted.push((animation, found));
        }
    }
    // Attacks for a body on its own clips (a retargeted one swings and aims
    // JA's way), and for a creature model on JA's skeleton. A body on its
    // own clips that is not a creature model walks and runs with a rifle
    // held up (HK-47).
    let own = !asset_ja::rig::is_humanoid(body);
    let attacks = creature || own;
    let engine = crate::acting::ready_names(creature)
        .chain(crate::acting::draw_names())
        .chain(crate::acting::flinch_names(creature))
        .chain(
            attacks
                .then(|| crate::acting::attack_names(creature))
                .into_iter()
                .flatten(),
        )
        .chain(
            (own && !creature)
                .then(crate::acting::rifle_gait_names)
                .into_iter()
                .flatten(),
        )
        // A retargeted body swings a stun baton with KOTOR's own swings.
        .chain(
            (!attacks)
                .then(crate::acting::baton_names)
                .into_iter()
                .flatten(),
        );
    for (animation, name) in engine {
        if let Some(found) = clip(cache, resolver, game, body, &name)? {
            acted.push((animation, found));
        }
    }
    // `animations.2da`'s rows by number (TSL's scripts play them so), the
    // ones above first.
    let rows = creature::animation_rows(&ResolverSource(resolver));
    for (animation, name) in creature::row_clips(&rows) {
        if acted.iter().any(|(known, _)| *known == animation) {
            continue;
        }
        let own = format!("c{name}");
        if let Some(found) = creature_clip(cache, resolver, game, body, creature, &own, name)? {
            acted.push((animation, found));
        }
    }
    Ok(acted)
}

/// A creature's clip: a creature model (types S and L) plays its `c…` name
/// (`cpause1`) when its chain has it; otherwise the shared name (`pause1`)
/// is played. reone and xoreos ask a creature model only for the `c…`
/// name; TSL's tank, mine and construction droids ship their idle only as
/// `pause1` (QUIRKS Q-0136).
pub fn creature_clip(
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    game: Game,
    model: &ModelIr,
    creature_type: bool,
    creature: &str,
    shared: &str,
) -> Result<Option<Arc<AnimationIr>>, String> {
    if creature_type && let Some(found) = clip(cache, resolver, game, model, creature)? {
        return Ok(Some(found));
    }
    clip(cache, resolver, game, model, shared)
}

/// The first supermodel in a model's chain that does not load, if any.
pub(crate) fn broken_chain(
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    model: &ModelIr,
) -> Result<Option<String>, String> {
    let mut seen = vec![model.name.to_ascii_lowercase()];
    let mut next = next_supermodel(model, &mut seen);
    while let Some(super_name) = next {
        let Some(parent) = read_into(cache, resolver, &super_name)? else {
            return Ok(Some(super_name));
        };
        next = next_supermodel(parent, &mut seen);
    }
    Ok(None)
}

/// The supermodel a chain steps to next: none at `null`, an empty name, or
/// a model already seen.
fn next_supermodel(model: &ModelIr, seen: &mut Vec<String>) -> Option<String> {
    let name = model.supermodel.as_deref()?.trim().to_ascii_lowercase();
    if name.is_empty() || name == "null" || seen.contains(&name) {
        return None;
    }
    seen.push(name.clone());
    Some(name)
}
