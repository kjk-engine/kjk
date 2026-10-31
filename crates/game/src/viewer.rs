//! Loads what the viewer draws through the resolver: one model (`kjk view`)
//! or every room of a module at its layout position (`kjk module`), with the
//! textures their meshes name.

use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::creature;
use asset_odyssey::hooks;
use asset_odyssey::model::{Matrix, ModelIr};
use asset_odyssey::module::{AreaGrass, AreaWeather, Kind, ModuleIr, Source};
use asset_odyssey::texture::TextureIr;
use asset_odyssey::weather::Fall;
use bevy::log::warn;
use bevy::math::Vec3;
use collision_odyssey::Surfaces;
use fmt_odyssey::ResType;
use fmt_odyssey::bwm::Walkmesh;
use fmt_odyssey::mdl::Model;
use fmt_odyssey::texture::Texture;
use fmt_odyssey::txi::Txi;
use vfs::resolver::Resolver;
use vfs::{Discovery, Game};

use crate::material::{AreaLight, Fog};
use crate::model_fx::{Chunk, ModelFx};
use crate::weapons::{Arms, Owner};

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
    let mut clips: BTreeMap<String, Arc<AnimationIr>> = ir
        .animations
        .iter()
        .map(|clip| (clip.name.to_ascii_lowercase(), Arc::new(clip.clone())))
        .collect();
    for name in crate::own_fx::look_clips() {
        if clips.contains_key(&name) {
            continue;
        }
        match clip(cache, resolver, game, ir, &name) {
            Ok(Some(found)) => {
                clips.insert(name, found);
            }
            Ok(None) => {}
            Err(why) => warn!("{}: its {name} clip is not read: {why}", ir.name),
        }
    }
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
    /// purp and turq, TSL's dgrn and silv too). JA has no gold, turquoise
    /// or silver: orange, blue and blue stand in; any other name is blue
    /// [verify: the texture's own colour would decide one we do not know].
    pub fn of_texture(texture: &str) -> SaberColor {
        let texture = texture.to_ascii_lowercase();
        [
            ("red", SaberColor::Red),
            ("gold", SaberColor::Orange),
            ("yelo", SaberColor::Yellow),
            ("gren", SaberColor::Green),
            ("dgrn", SaberColor::Green),
            ("purp", SaberColor::Purple),
        ]
        .into_iter()
        .find(|(name, _)| texture.contains(name))
        .map_or(SaberColor::Blue, |(_, color)| color)
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
    let loose = resolver
        .find(name, ResType::TXI)
        .and_then(|found| found.location.read().ok())
        .map(|bytes| Txi::parse(&bytes));
    let parsed = match resolver.find(name, ResType::TPC) {
        Some(found) => Texture::parse_tpc(&found.location.read().ok()?).ok()?,
        None => {
            Texture::parse_tga(&resolver.find(name, ResType::TGA)?.location.read().ok()?).ok()?
        }
    };
    let alpha = matches!(
        parsed.format,
        fmt_odyssey::texture::PixelFormat::Rgba8 | fmt_odyssey::texture::PixelFormat::Dxt5
    );
    let facts = asset_odyssey::texture::TextureFacts::from_txi(
        loose.as_ref().unwrap_or(&parsed.txi),
        alpha,
    );
    let cube = parsed.cube && parsed.layers.len() == 6;
    let frames = if parsed.cube { 1 } else { parsed.layers.len() };
    Some((facts, cube, frames))
}

/// The texture the games would pick for a name: TPC first, then TGA.
pub(crate) fn texture(resolver: &Resolver, name: &str) -> Option<TextureIr> {
    let loose = resolver
        .find(name, ResType::TXI)
        .and_then(|found| found.location.read().ok())
        .map(|bytes| Txi::parse(&bytes));
    if let Some(found) = resolver.find(name, ResType::TPC) {
        let bytes = found.location.read().ok()?;
        return TextureIr::from_texture(&Texture::parse_tpc(&bytes).ok()?, loose.as_ref());
    }
    let found = resolver.find(name, ResType::TGA)?;
    let bytes = found.location.read().ok()?;
    TextureIr::from_texture(&Texture::parse_tga(&bytes).ok()?, loose.as_ref())
}

pub fn model(resolver: &Resolver, name: &str) -> Result<Option<ModelIr>, String> {
    Ok(parsed(resolver, name)?.map(|parsed| ModelIr::from_model(&parsed)))
}

/// A model's MDL and MDX as parsed, before they become a `ModelIr`.
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

pub fn parsed(resolver: &Resolver, name: &str) -> Result<Option<Model>, String> {
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
/// self-illumination those the clip ends on, the self-illumination
/// `glow` instead when given. A detected mine is drawn so: its `detect`
/// clip ends with its trigger's dome raised, lit and opaque (`made`).
pub(crate) fn held(model: &mut Model, clip: &str, glow: Option<[f32; 3]>) {
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
    fn pose(
        node: &mut fmt_odyssey::mdl::Node,
        keys: &BTreeMap<String, Vec<(u32, Vec<f32>)>>,
        glow: Option<[f32; 3]>,
    ) {
        for (kind, row) in keys
            .get(&node.name.to_ascii_lowercase())
            .into_iter()
            .flatten()
        {
            let row = match (*kind, glow) {
                (POSITION, _) if row.len() == 3 => {
                    for (at, by) in node.position.iter_mut().zip(row) {
                        *at += by;
                    }
                    continue;
                }
                (SELF_ILLUM, Some(glow)) => glow.to_vec(),
                (POSITION, _) => continue,
                _ => row.clone(),
            };
            let rest = fmt_odyssey::mdl::Controller {
                kind: *kind,
                columns: row.len() as u8,
                bezier: false,
                times: vec![0.0],
                values: row,
            };
            match node.controllers.iter_mut().find(|c| c.kind == *kind) {
                Some(controller) => *controller = rest,
                None => node.controllers.push(rest),
            }
        }
        for child in &mut node.children {
            pose(child, keys, glow);
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
    pose(&mut model.root, &keys, glow);
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
    let names = models
        .into_iter()
        .flat_map(|placed| &placed.ir.meshes)
        .flat_map(|mesh| [mesh.texture.as_ref(), mesh.lightmap.as_ref()])
        .flatten();
    for name in names {
        load(name, &mut textures);
    }
    // Env maps the loaded textures' TXI names.
    let envmaps: Vec<String> = textures
        .values()
        .filter_map(|texture: &TextureIr| texture.facts.envmap.clone())
        .collect();
    for name in &envmaps {
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
    }];
    let (textures, missing) = textures(&resolver, &models);
    Ok(LoadedScene {
        name: name.to_string(),
        models,
        textures,
        missing,
        missing_body_textures: Vec::new(),
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

    fn exists(&self, resref: &str, restype: ResType) -> bool {
        self.0.find(resref, restype).is_some()
    }

    fn talk(&self) -> Option<std::sync::Arc<fmt_odyssey::tlk::Tlk>> {
        self.0.talk.get()
    }
}

/// Every room of the module's entry area at its layout position.
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
    for room in &area.rooms {
        let index = match parsed(resolver, &room.model)? {
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
            floor,
            floor_error,
        });
    }
    laps.lap("rooms");
    // Doors and placeables at their GIT position and bearing (as xoreos places them): doors in their rest pose, placeables posed by
    // their state's clip with their emitters (`own_fx`).
    let mut creatures = CreatureLoad::default();
    let loaded = &mut creatures.models;
    let mut instances = 0;
    let mut doors = Vec::new();
    let mut instance_models = BTreeMap::new();
    // Each placeable model's own clips and emitters, read once.
    let mut fx_of: BTreeMap<String, Option<OwnFx>> = BTreeMap::new();
    for (object, instance) in area.instances.iter().enumerate() {
        let Some(name) = &instance.model else {
            continue;
        };
        if !loaded.contains_key(name) {
            let parsed = parsed(resolver, name)?;
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
            });
        }
        match &loaded[name] {
            Some(ir) => {
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
    laps.lap("textures");
    missing.extend(creatures.missing_body_textures.iter().cloned());
    Ok(LoadedScene {
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
        self.place_look(
            (resolver, game),
            (object, template, look),
            (position, facing),
            models,
        )
    }

    /// A creature of `appearance.2da` row `appearance` wearing `armour` (a
    /// UTI) or nothing on its body, with the head of `heads.2da` row `head`
    /// where one is given (else the row's own), as `place` places one of a
    /// template.
    pub fn place_appearance(
        &mut self,
        (resolver, game): (&Resolver, Game),
        (appearance, armour, head): (u32, Option<&str>, Option<u8>),
        (position, facing): ([f32; 3], f32),
        models: &mut Vec<Placed>,
    ) -> Result<Option<CreatureView>, String> {
        let name = format!("appearance {appearance}");
        let source = ResolverSource(resolver);
        let mut look = creature::resolve_appearance(
            &source,
            appearance,
            armour.map(str::to_string),
            &mut self.tables,
        )
        .map_err(|refusal| format!("{name}: {refusal:?}"))?;
        if let Some(head) = head {
            look.head = (self.tables.get(&source, "heads"))
                .and_then(|heads| heads.resref(usize::from(head), "head"));
        }
        self.place_look(
            (resolver, game),
            (0, &name, look),
            (position, facing),
            models,
        )
    }

    /// `look` (`template`'s, as the notes name it) placed as `place` says.
    fn place_look(
        &mut self,
        (resolver, game): (&Resolver, Game),
        (object, template, look): (u32, &str, creature::CreatureLook),
        (position, facing): ([f32; 3], f32),
        models: &mut Vec<Placed>,
    ) -> Result<Option<CreatureView>, String> {
        let loaded = &mut self.models;
        let Some(mut body) = cached(loaded, resolver, &look.body)? else {
            self.missing_models.push(look.body.clone());
            return Ok(None);
        };
        retexture(&mut body, look.body_texture.as_deref());
        if let Some(amount) = look.chest {
            asset_odyssey::body_morph::apply(&mut body, amount);
        }
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
        models.push(Placed {
            clip: idle.clone(),
            ..at(body)
        });
        self.placed += 1;
        if let (Some(head), Some(hook), Some(hook_node)) = (&look.head, hook, hook_node) {
            match cached(loaded, resolver, head)? {
                Some(ir) => models.push(Placed {
                    attach: Some(hook),
                    hook: Some((body_index, hook_node)),
                    clip: idle.clone(),
                    ..at(ir)
                }),
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

/// A model through a per-load cache.
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
    let mut seen = vec![model.name.to_ascii_lowercase()];
    let mut next = next_supermodel(model, &mut seen);
    while let Some(super_name) = next {
        let Some(parent) = read_into(cache, resolver, &super_name)? else {
            break;
        };
        if let Some(found) = find(parent) {
            return Ok(Some(found));
        }
        next = next_supermodel(parent, &mut seen);
    }
    Ok(None)
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
    let engine = crate::acting::ready_names(creature)
        .chain(crate::acting::draw_names())
        .chain(crate::acting::flinch_names(creature))
        .chain(
            creature
                .then(crate::acting::attack_names)
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
