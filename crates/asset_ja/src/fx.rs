//! JA's effect files (`.efx`) at run time: what playing one spawns, how a
//! spawned primitive looks in a given frame, and the geometry it is drawn
//! with.
//!
//! An effect is a list of primitive templates. Playing it picks each
//! template's values (counts, delays, offsets, motion, colours, sizes,
//! lives, shaders) and gives back the spawned primitives; the caller keeps
//! them and asks each frame where they are and how they look. The kinds
//! run here are sprites (particles), lines, lightning (electricity),
//! sounds, oriented particles, tails, cylinders, decals and runners (an
//! effect that plays another one).
//!
//! Every pick comes from a seeded generator the caller owns ([`Rng`]), the
//! game's own linear congruential one, so a caller that keeps one generator
//! per source gets the same effect for the same seed. A particle with
//! physics bounces off the level along a traced path (QUIRKS.md Q-0076):
//! see [`Spawned::collide`].
//!
//! Not supported, and reported in [`Effect::unsupported`]: the other
//! primitive kinds, impact and death effects, bodies, wind, and `relative`
//! (an effect that follows what played it). A delayed copy's values are
//! picked when the effect is played, not when the copy starts.

use fmt_ja::efx::Group;
use glam::{Quat, Vec3};

// --- The flag sets ---------------------------------------------------------

/// A primitive's flags: its own keywords, and per transition channel how
/// the channel moves from its start value to its end value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags(u32);

/// The transition channels of a primitive, each with its own settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    Rgb,
    Alpha,
    Size,
    Length,
    Size2,
}

impl Channel {
    const ALL: [Channel; 5] = [
        Channel::Rgb,
        Channel::Alpha,
        Channel::Size,
        Channel::Length,
        Channel::Size2,
    ];

    fn name(self) -> &'static str {
        match self {
            Channel::Rgb => "rgb",
            Channel::Alpha => "alpha",
            Channel::Size => "size",
            Channel::Length => "length",
            Channel::Size2 => "size2",
        }
    }

    /// The channel a primitive's subgroup sets, by the subgroup's name in
    /// lower case.
    fn from_group(name: &str) -> Option<Channel> {
        match name {
            "rgb" => Some(Channel::Rgb),
            "alpha" => Some(Channel::Alpha),
            "size" | "width" => Some(Channel::Size),
            "length" | "height" => Some(Channel::Length),
            "size2" | "width2" => Some(Channel::Size2),
            _ => None,
        }
    }

    /// The first of the channel's four members' bits.
    const fn shift(self) -> u32 {
        CHANNEL_BASE + 4 * self as u32
    }
}

/// The four members of a channel, in bit order.
const MEMBERS: [&str; 4] = ["linear", "random", "nonlinear", "wave"];
const LINEAR: u32 = 0;
const RANDOM: u32 = 1;
const NONLINEAR: u32 = 2;
const WAVE: u32 = 3;

/// The primitive keywords, in bit order.
const KEYWORDS: [&str; 7] = [
    "usemodel",
    "usebbox",
    "usephysics",
    "expensivephysics",
    "usealpha",
    "relative",
    "setshadertime",
];
/// Where the channels' bits begin, after the keywords'.
const CHANNEL_BASE: u32 = KEYWORDS.len() as u32;

/// How a channel's value moves over the primitive's life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Straight (if linear) or held.
    None,
    /// Held, then eased out from the parm's time.
    NonLinear,
    /// Swinging, the parm its speed.
    Wave,
    /// Eased in until the parm's time, then held at the end.
    Clamp,
}

impl Flags {
    pub const NONE: Flags = Flags(0);
    /// With lightning, `taper`: the bolt narrows toward its end.
    pub const USE_MODEL: Flags = Flags(1 << 0);
    /// With lightning, `grow`: the bolt reaches its end over its life.
    pub const USE_BBOX: Flags = Flags(1 << 1);
    /// The particle bounces off the level; with lightning, `branch`.
    pub const USE_PHYSICS: Flags = Flags(1 << 2);
    pub const EXPENSIVE_PHYSICS: Flags = Flags(1 << 3);
    /// The alpha goes to the alpha byte rather than darkening the colour.
    pub const USE_ALPHA: Flags = Flags(1 << 4);
    pub const RELATIVE: Flags = Flags(1 << 5);
    /// The shader's time starts with the primitive.
    pub const SET_SHADER_TIME: Flags = Flags(1 << 6);

    const fn member(channel: Channel, member: u32) -> Flags {
        Flags(1 << (channel.shift() + member))
    }

    /// All of `other`'s members are present.
    pub fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }

    fn has(self, channel: Channel, member: u32) -> bool {
        self.contains(Flags::member(channel, member))
    }

    fn shape(self, channel: Channel) -> Shape {
        match (self.has(channel, NONLINEAR), self.has(channel, WAVE)) {
            (false, false) => Shape::None,
            (true, false) => Shape::NonLinear,
            (false, true) => Shape::Wave,
            (true, true) => Shape::Clamp,
        }
    }

    /// The members by their canonical names (`usealpha`, `size.linear`, …),
    /// in byte order.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = KEYWORDS
            .iter()
            .enumerate()
            .filter(|&(bit, _)| self.0 & (1 << bit) != 0)
            .map(|(_, name)| (*name).to_string())
            .collect();
        for channel in Channel::ALL {
            for (member, name) in MEMBERS.iter().enumerate() {
                if self.has(channel, member as u32) {
                    names.push(format!("{}.{name}", channel.name()));
                }
            }
        }
        names.sort();
        names
    }
}

impl std::ops::BitOr for Flags {
    type Output = Flags;

    fn bitor(self, other: Flags) -> Flags {
        Flags(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for Flags {
    fn bitor_assign(&mut self, other: Flags) {
        self.0 |= other.0;
    }
}

/// The flags that only matter when a primitive is spawned: where its
/// origin, end and axis come from, and how its motion and colour are
/// picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpawnFlags(u16);

/// The spawn keywords, in bit order.
const SPAWN_KEYWORDS: [&str; 10] = [
    "org2fromtrace",
    "orgonsphere",
    "axisfromsphere",
    "orgoncylinder",
    "org2isoffset",
    "cheaporgcalc",
    "cheaporg2calc",
    "absolutevel",
    "absoluteaccel",
    "rgbcomponentinterpolation",
];

impl SpawnFlags {
    pub const NONE: SpawnFlags = SpawnFlags(0);
    /// The end point is where a line along the axis meets the level.
    pub const END_FROM_TRACE: SpawnFlags = SpawnFlags(1 << 0);
    /// The origin is moved to a point on an ellipsoid.
    pub const ON_SPHERE: SpawnFlags = SpawnFlags(1 << 1);
    /// The axis points out of the sphere or cylinder through the origin.
    pub const AXIS_OUTWARD: SpawnFlags = SpawnFlags(1 << 2);
    /// The origin is moved to a point on a cylinder round the axis.
    pub const ON_CYLINDER: SpawnFlags = SpawnFlags(1 << 3);
    /// The traced end point is moved by `origin2`.
    pub const TRACE_OFFSET: SpawnFlags = SpawnFlags(1 << 4);
    /// The offset is in world space, not along the axis.
    pub const WORLD_OFFSET: SpawnFlags = SpawnFlags(1 << 5);
    /// The end point's offset is in world space, not along the axis.
    pub const WORLD_END_OFFSET: SpawnFlags = SpawnFlags(1 << 6);
    /// The velocity is in world space.
    pub const WORLD_VELOCITY: SpawnFlags = SpawnFlags(1 << 7);
    /// The acceleration is in world space.
    pub const WORLD_ACCELERATION: SpawnFlags = SpawnFlags(1 << 8);
    /// One fraction picks every colour component.
    pub const ONE_COLOUR_FRACTION: SpawnFlags = SpawnFlags(1 << 9);

    /// All of `other`'s members are present.
    pub fn contains(self, other: SpawnFlags) -> bool {
        self.0 & other.0 == other.0
    }

    /// The members by their keywords, in byte order.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = SPAWN_KEYWORDS
            .iter()
            .enumerate()
            .filter(|&(bit, _)| self.0 & (1 << bit) != 0)
            .map(|(_, name)| (*name).to_string())
            .collect();
        names.sort();
        names
    }
}

impl std::ops::BitOr for SpawnFlags {
    type Output = SpawnFlags;

    fn bitor(self, other: SpawnFlags) -> SpawnFlags {
        SpawnFlags(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for SpawnFlags {
    fn bitor_assign(&mut self, other: SpawnFlags) {
        self.0 |= other.0;
    }
}

// --- The generator ---------------------------------------------------------

/// The game's seeded generator: a linear congruential step on a 32-bit
/// state, whose low 16 bits make each number. Every method takes one step.
#[derive(Debug, Clone, Copy)]
pub struct Rng(pub i32);

impl Rng {
    fn step(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(69069).wrapping_add(1);
        self.0
    }

    /// A number in [0, 1), in steps of 1/65536.
    pub fn random(&mut self) -> f32 {
        (self.step() & 0xffff) as f32 / 65536.0
    }

    /// A whole number from `low` to `high`, both included (`low` when
    /// `high` is below it).
    pub fn roll_between(&mut self, low: i32, high: i32) -> i32 {
        let span = high.wrapping_sub(low).wrapping_add(1).max(1);
        let pick = (self.random() * span as f32) as i32;
        low.wrapping_add(pick.min(span - 1))
    }

    /// A number in [-1, 1).
    pub fn signed_unit(&mut self) -> f32 {
        2.0 * (self.random() - 0.5)
    }

    /// A number from `min` toward `max`.
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        let r = self.random();
        min + (r * (max - min))
    }
}

/// How far a traced end point looks (units).
pub const MAX_TRACE: f32 = 16384.0;

// --- Templates --------------------------------------------------------------

/// A value picked between two bounds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Range {
    pub min: f32,
    pub max: f32,
}

impl Range {
    const fn at(value: f32) -> Range {
        Range {
            min: value,
            max: value,
        }
    }

    fn val(&self, rng: &mut Rng) -> f32 {
        if self.min == self.max {
            self.min
        } else {
            rng.range(self.min, self.max)
        }
    }

    fn val_at(&self, fraction: f32) -> f32 {
        if self.min == self.max {
            self.min
        } else {
            self.min + fraction * (self.max - self.min)
        }
    }
}

/// The primitive kinds this module runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Particle,
    Line,
    Electricity,
    /// A sound: its file is the spawned primitive's `shader`.
    Sound,
    /// A particle drawn square to its `normal` rather than to the view.
    OrientedParticle,
    /// A moving line, `length` long, trailing behind where it goes.
    Tail,
    /// A tube along its `normal`, `size` round at its start and `size2` at
    /// its far end, `length` long.
    Cylinder,
    /// A mark laid on the surface square to `normal`, `size` (its start)
    /// round; it stays ([`mark`]).
    Decal,
    /// Another effect, played at its origin along `normal`: the effect's
    /// name is the spawned primitive's `shader`.
    Runner,
}

impl Kind {
    /// Kinds that move by velocity and acceleration.
    fn moving(self) -> bool {
        matches!(self, Kind::Particle | Kind::OrientedParticle | Kind::Tail)
    }

    /// Kinds drawn between two points.
    fn two_point(self) -> bool {
        matches!(self, Kind::Line | Kind::Electricity)
    }

    /// Kinds that turn.
    fn turning(self) -> bool {
        matches!(self, Kind::Particle | Kind::OrientedParticle | Kind::Decal)
    }
}

/// One primitive of an effect file (the fields the supported kinds read).
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub kind: Kind,
    pub count: Range,
    pub life: Range,
    pub delay: Range,
    pub flags: Flags,
    pub spawn_flags: SpawnFlags,
    pub origin: [Range; 3],
    pub origin2: [Range; 3],
    pub velocity: [Range; 3],
    pub acceleration: [Range; 3],
    pub gravity: Range,
    pub rotation: Range,
    pub rotation_delta: Range,
    /// `intensity` (or `bounce`): electricity's chaos.
    pub elasticity: Range,
    pub rgb_start: [Range; 3],
    pub rgb_end: [Range; 3],
    pub rgb_parm: Range,
    pub alpha_start: Range,
    pub alpha_end: Range,
    pub alpha_parm: Range,
    pub size_start: Range,
    pub size_end: Range,
    pub size_parm: Range,
    pub shaders: Vec<String>,
    /// A sound's files.
    pub sounds: Vec<String>,
    /// A tail's or cylinder's length, and a cylinder's far size.
    pub length_start: Range,
    pub length_end: Range,
    pub length_parm: Range,
    pub size2_start: Range,
    pub size2_end: Range,
    pub size2_parm: Range,
    /// A runner's effects (one is picked), without `effects/` and `.efx`.
    pub play: Vec<String>,
    /// The ellipsoid or cylinder an origin is picked on (`orgOnSphere`,
    /// `orgOnCylinder`).
    pub radius: Range,
    pub height: Range,
}

impl Template {
    /// A primitive of `kind` with the values a file leaves unsaid.
    pub fn new(kind: Kind) -> Template {
        let zero = [Range::default(); 3];
        let white = [Range::at(1.0); 3];
        Template {
            kind,
            count: Range::at(1.0),
            life: Range::at(50.0),
            delay: Range::default(),
            flags: Flags::NONE,
            spawn_flags: SpawnFlags::NONE,
            origin: zero,
            origin2: zero,
            velocity: zero,
            acceleration: zero,
            gravity: Range::default(),
            rotation: Range::default(),
            rotation_delta: Range::default(),
            elasticity: Range::at(0.1),
            rgb_start: white,
            rgb_end: white,
            rgb_parm: Range::default(),
            alpha_start: Range::at(1.0),
            alpha_end: Range::at(1.0),
            alpha_parm: Range::default(),
            size_start: Range::at(1.0),
            size_end: Range::at(1.0),
            size_parm: Range::default(),
            shaders: Vec::new(),
            sounds: Vec::new(),
            length_start: Range::at(1.0),
            length_end: Range::at(1.0),
            length_parm: Range::default(),
            size2_start: Range::at(1.0),
            size2_end: Range::at(1.0),
            size2_parm: Range::default(),
            play: Vec::new(),
            radius: Range::default(),
            height: Range::default(),
        }
    }

    /// A channel's start, end and parm fields (rgb's start and end are
    /// per component, so are not here).
    fn channel(&mut self, channel: Channel) -> (&mut Range, &mut Range, &mut Range) {
        match channel {
            Channel::Rgb => unreachable!("rgb's start and end are vectors"),
            Channel::Alpha => (
                &mut self.alpha_start,
                &mut self.alpha_end,
                &mut self.alpha_parm,
            ),
            Channel::Size => (
                &mut self.size_start,
                &mut self.size_end,
                &mut self.size_parm,
            ),
            Channel::Length => (
                &mut self.length_start,
                &mut self.length_end,
                &mut self.length_parm,
            ),
            Channel::Size2 => (
                &mut self.size2_start,
                &mut self.size2_end,
                &mut self.size2_parm,
            ),
        }
    }

    /// Reads one channel subgroup (`rgb { start … end … flags … }`),
    /// pushing what it does not know to `unsupported`.
    fn read_channel(&mut self, group: &str, sub: &Group, unsupported: &mut Vec<String>) {
        let name = sub.name.to_ascii_lowercase();
        let Some(channel) = Channel::from_group(&name) else {
            unsupported.push(format!("{group} {name}"));
            return;
        };
        for (key, value) in &sub.pairs {
            let key = key.to_ascii_lowercase();
            match key.as_str() {
                "flags" | "flag" => {
                    for word in value.split_whitespace() {
                        let members: &[u32] = match word.to_ascii_lowercase().as_str() {
                            "linear" => &[LINEAR],
                            "random" => &[RANDOM],
                            "nonlinear" => &[NONLINEAR],
                            "wave" => &[WAVE],
                            "clamp" => &[NONLINEAR, WAVE],
                            _ => &[],
                        };
                        for &member in members {
                            self.flags |= Flags::member(channel, member);
                        }
                    }
                }
                "start" | "end" => {
                    let start = key == "start";
                    if channel == Channel::Rgb {
                        if let Some(v) = vector(value) {
                            *if start {
                                &mut self.rgb_start
                            } else {
                                &mut self.rgb_end
                            } = v;
                        }
                    } else if let Some(r) = floats(value) {
                        let (s, e, _) = self.channel(channel);
                        *if start { s } else { e } = r;
                    }
                }
                "parm" | "parms" => {
                    let r = floats(value).unwrap_or(Range::at(0.0));
                    if channel == Channel::Rgb {
                        self.rgb_parm = r;
                    } else {
                        *self.channel(channel).2 = r;
                    }
                }
                _ => unsupported.push(format!("{group} {name} {key}")),
            }
        }
    }
}

/// An effect file's supported primitives, and what it asks for that is
/// not supported.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Effect {
    pub templates: Vec<Template>,
    pub unsupported: Vec<String>,
}

/// An effect's name as the game plays it: lower case, no `effects/`
/// before it nor `.efx` after.
pub fn effect_name(name: &str) -> String {
    let name = fmt_ja::game_path(name);
    let name = name.strip_prefix("effects/").unwrap_or(&name);
    name.strip_suffix(".efx").unwrap_or(name).to_string()
}

/// Up to two numbers (a range; one is both ends), as the game reads them.
fn floats(value: &str) -> Option<Range> {
    let mut numbers = value.split_whitespace().map(str::parse::<f32>);
    let min = numbers.next()?.ok()?;
    let max = numbers.next().and_then(Result::ok).unwrap_or(min);
    Some(Range { min, max })
}

/// Three numbers, or six (a range per axis), as the game reads a vector.
fn vector(value: &str) -> Option<[Range; 3]> {
    let numbers: Vec<f32> = value
        .split_whitespace()
        .map_while(|w| w.parse::<f32>().ok())
        .collect();
    match numbers.len() {
        3 => Some([0, 1, 2].map(|i| Range::at(numbers[i]))),
        n if n >= 6 => Some([0, 1, 2].map(|i| Range {
            min: numbers[i],
            max: numbers[i + 3],
        })),
        _ => None,
    }
}

/// An effect file's primitives as templates.
pub fn effect(root: &Group) -> Effect {
    let mut out = Effect::default();
    for group in &root.groups {
        let kind = match group.name.to_ascii_lowercase().as_str() {
            "particle" => Kind::Particle,
            "line" => Kind::Line,
            "electricity" => Kind::Electricity,
            "sound" => Kind::Sound,
            "orientedparticle" => Kind::OrientedParticle,
            "tail" => Kind::Tail,
            "cylinder" => Kind::Cylinder,
            "decal" => Kind::Decal,
            "fxrunner" => Kind::Runner,
            other => {
                out.unsupported.push(format!("primitive {other}"));
                continue;
            }
        };
        let mut t = Template::new(kind);
        for (key, value) in &group.pairs {
            let key = key.to_ascii_lowercase();
            let range = floats(value);
            let known = match (key.as_str(), range) {
                ("count", Some(r)) => {
                    t.count = r;
                    true
                }
                ("life", Some(r)) => {
                    t.life = r;
                    true
                }
                ("delay", Some(r)) => {
                    t.delay = r;
                    true
                }
                ("bounce" | "intensity", Some(r)) => {
                    t.elasticity = r;
                    true
                }
                ("rotation", Some(r)) => {
                    t.rotation = r;
                    true
                }
                ("rotationdelta", Some(r)) => {
                    t.rotation_delta = r;
                    true
                }
                ("gravity", Some(r)) => {
                    t.gravity = r;
                    true
                }
                ("radius", Some(r)) => {
                    t.radius = r;
                    true
                }
                ("height", Some(r)) => {
                    t.height = r;
                    true
                }
                ("origin", _) => vector(value).map(|v| t.origin = v).is_some(),
                ("origin2", _) => vector(value).map(|v| t.origin2 = v).is_some(),
                ("velocity" | "vel", _) => vector(value).map(|v| t.velocity = v).is_some(),
                ("acceleration" | "accel", _) => {
                    vector(value).map(|v| t.acceleration = v).is_some()
                }
                ("flags" | "flag", _) => {
                    for flag in value.split_whitespace() {
                        let lower = flag.to_ascii_lowercase();
                        match KEYWORDS.iter().position(|k| *k == lower) {
                            Some(bit) => t.flags |= Flags(1 << bit),
                            None => out.unsupported.push(format!("flag {lower}")),
                        }
                    }
                    true
                }
                ("spawnflags" | "spawnflag", _) => {
                    for flag in value.split_whitespace() {
                        let lower = flag.to_ascii_lowercase();
                        match SPAWN_KEYWORDS.iter().position(|k| *k == lower) {
                            Some(bit) => t.spawn_flags |= SpawnFlags(1 << bit),
                            None => out.unsupported.push(format!("spawn flag {lower}")),
                        }
                    }
                    true
                }
                ("name" | "cullrange", _) => true,
                _ => false,
            };
            if !known {
                out.unsupported.push(format!("{} {key}", group.name));
            }
        }
        for (key, items) in &group.lists {
            match key.to_ascii_lowercase().as_str() {
                "shaders" | "shader" => t.shaders.extend(items.iter().cloned()),
                "sounds" | "sound" => t
                    .sounds
                    .extend(items.iter().map(|s| s.to_ascii_lowercase())),
                "playfx" => t.play.extend(items.iter().map(|s| effect_name(s))),
                other => out.unsupported.push(format!("{} {other}", group.name)),
            }
        }
        if let Some(value) = group.pair("shader").or(group.pair("shaders")) {
            t.shaders.push(value.to_string());
        }
        for sub in &group.groups {
            t.read_channel(&group.name, sub, &mut out.unsupported);
        }
        if t.flags.contains(Flags::RELATIVE) {
            out.unsupported.push(format!("{} relative", group.name));
        }
        out.templates.push(t);
    }
    out
}

// --- Playing an effect -----------------------------------------------------

/// A primitive once spawned, with the values its template picked.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawned {
    pub kind: Kind,
    pub start_ms: f32,
    pub end_ms: f32,
    pub origin: Vec3,
    /// A line's or electricity's end.
    pub origin2: Vec3,
    pub velocity: Vec3,
    pub acceleration: Vec3,
    /// Start, end, and the parm as the primitive keeps it.
    pub size: [f32; 3],
    pub alpha: [f32; 3],
    pub rgb_start: Vec3,
    pub rgb_end: Vec3,
    pub rgb_parm: f32,
    /// The way its effect was played along (its axis' forward): an oriented
    /// particle's, cylinder's and decal's face, a runner's way.
    pub normal: Vec3,
    /// A tail's or cylinder's length, a cylinder's far size: start, end,
    /// parm.
    pub length: [f32; 3],
    pub size2: [f32; 3],
    pub rotation: f32,
    pub rotation_delta: f32,
    /// Electricity's chaos; a physics particle's bounce (`elasticity`).
    pub chaos: f32,
    /// A physics particle's way through the level as far as it is traced
    /// (`collide`); `None` for one that flies through it.
    pub path: Option<Box<Path>>,
    /// The shader it is drawn with; a sound's file.
    pub shader: String,
    pub flags: Flags,
    /// Electricity's path seed: the same bolt for the whole of its life.
    pub seed: i32,
}

/// `v` along the axes: `axis[0] * v.x + axis[1] * v.y + axis[2] * v.z`.
fn along(axis: [Vec3; 3], v: Vec3) -> Vec3 {
    axis[0] * v.x + axis[1] * v.y + axis[2] * v.z
}

/// Picks a vector, x then y then z.
fn pick(ranges: &[Range; 3], rng: &mut Rng) -> Vec3 {
    let x = ranges[0].val(rng);
    let y = ranges[1].val(rng);
    let z = ranges[2].val(rng);
    Vec3::new(x, y, z)
}

/// Plays `effect` at `origin` along `axis` (forward, then the two sides)
/// at `now_ms`: every copy of every template, in order. `trace` is where a
/// line from the first point to the second meets the level (the second
/// point when nothing is in the way), for end points found by tracing.
pub fn play(
    effect: &Effect,
    origin: Vec3,
    axis: [Vec3; 3],
    now_ms: f32,
    rng: &mut Rng,
    trace: &dyn Fn(Vec3, Vec3) -> Vec3,
) -> Vec<Spawned> {
    let mut out = Vec::new();
    for template in &effect.templates {
        let count = template.count.val(rng);
        let mut copies = (count + 0.5).floor().max(0.0) as usize;
        if template.count.min >= 1.0 {
            copies = copies.max(1);
        }
        for _ in 0..copies {
            let delay = template.delay.val(rng).trunc().max(0.0);
            out.push(spawn(template, origin, axis, now_ms + delay, rng, trace));
        }
    }
    out
}

/// A channel's parm as the spawned primitive keeps it: a wave's speed per
/// millisecond, an easing's start as a time (the parm a percentage of the
/// life), otherwise as picked.
fn parm(flags: Flags, channel: Channel, picked: f32, start_ms: f32, life: f32) -> f32 {
    match flags.shape(channel) {
        Shape::Wave => (picked * std::f32::consts::PI) * 0.001,
        Shape::NonLinear | Shape::Clamp => ((picked * 0.01) * life) + start_ms,
        Shape::None => picked,
    }
}

/// One copy of `t`, started at `start_ms`. The order of the picks is the
/// game's, so a seed gives the same copy.
fn spawn(
    t: &Template,
    effect_origin: Vec3,
    effect_axis: [Vec3; 3],
    start_ms: f32,
    rng: &mut Rng,
    trace: &dyn Fn(Vec3, Vec3) -> Vec3,
) -> Spawned {
    let spawn_flags = t.spawn_flags;
    let world = |flag: SpawnFlags, axis: [Vec3; 3], v: Vec3| {
        if spawn_flags.contains(flag) {
            v
        } else {
            along(axis, v)
        }
    };

    // Where it starts, and the axis it moves along.
    let offset = pick(&t.origin, rng);
    let mut point = effect_origin + world(SpawnFlags::WORLD_OFFSET, effect_axis, offset);
    let mut axis = effect_axis;
    let axis_out = spawn_flags.contains(SpawnFlags::AXIS_OUTWARD);
    if spawn_flags.contains(SpawnFlags::ON_SPHERE) {
        let across = rng.range(0.0, 360.0).to_radians();
        let down = rng.range(0.0, 180.0).to_radians();
        let width = t.radius.val(rng);
        let height = t.height.val(rng);
        let on = Vec3::new(
            across.sin() * width * down.sin(),
            across.cos() * width * down.sin(),
            down.cos() * height,
        );
        point += on;
        if axis_out {
            axis = self::axis(on.normalize_or(axis[0]));
        }
    } else if spawn_flags.contains(SpawnFlags::ON_CYLINDER) {
        let radius = t.radius.val(rng);
        let up_to = rng.range(-1.0, 1.0);
        let height = t.height.val(rng);
        let mut on = axis[1] * radius + axis[0] * ((up_to * 0.5) * height);
        let turn = rng.range(0.0, 360.0).to_radians();
        on = Quat::from_axis_angle(axis[0], turn) * on;
        point += on;
        if axis_out {
            let forward = on.normalize_or(axis[0]);
            let up = if forward.z == 1.0 { Vec3::Y } else { Vec3::Z };
            let left = up.cross(forward);
            axis = [forward, left, forward.cross(left)];
        }
    }

    // How it moves.
    let (mut velocity, mut acceleration) = (Vec3::ZERO, Vec3::ZERO);
    if t.kind.moving() {
        let v = pick(&t.velocity, rng);
        velocity = world(SpawnFlags::WORLD_VELOCITY, axis, v);
        let a = pick(&t.acceleration, rng);
        acceleration = world(SpawnFlags::WORLD_ACCELERATION, axis, a);
        acceleration.z += t.gravity.val(rng);
    }

    // Where a line or bolt ends.
    let mut origin2 = Vec3::ZERO;
    if t.kind.two_point() {
        if spawn_flags.contains(SpawnFlags::END_FROM_TRACE) {
            let mut far = point + axis[0] * MAX_TRACE;
            if spawn_flags.contains(SpawnFlags::TRACE_OFFSET) {
                let o2 = pick(&t.origin2, rng);
                far += world(SpawnFlags::WORLD_END_OFFSET, axis, o2);
            }
            origin2 = trace(point, far);
        } else {
            let o2 = pick(&t.origin2, rng);
            origin2 = effect_origin + world(SpawnFlags::WORLD_END_OFFSET, axis, o2);
        }
    }

    // How it looks.
    let (rgb_start, rgb_end) = if spawn_flags.contains(SpawnFlags::ONE_COLOUR_FRACTION) {
        let f = rng.random();
        let at = |r: &[Range; 3]| Vec3::new(r[0].val_at(f), r[1].val_at(f), r[2].val_at(f));
        (at(&t.rgb_start), at(&t.rgb_end))
    } else {
        let start = pick(&t.rgb_start, rng);
        (start, pick(&t.rgb_end, rng))
    };
    let triple = |s: &Range, e: &Range, p: &Range, rng: &mut Rng| {
        let start = s.val(rng);
        let end = e.val(rng);
        [start, end, p.val(rng)]
    };
    let mut size = triple(&t.size_start, &t.size_end, &t.size_parm, rng);
    let mut alpha = triple(&t.alpha_start, &t.alpha_end, &t.alpha_parm, rng);
    let rgb_parm = t.rgb_parm.val(rng);
    let mut length = triple(&t.length_start, &t.length_end, &t.length_parm, rng);
    let mut size2 = triple(&t.size2_start, &t.size2_end, &t.size2_parm, rng);
    let (rotation, rotation_delta) = if t.kind.turning() {
        let rotation = t.rotation.val(rng);
        (rotation, t.rotation_delta.val(rng))
    } else {
        (0.0, 0.0)
    };
    let chaos = t.elasticity.val(rng);
    let life = t.life.val(rng).trunc();

    let names = match t.kind {
        Kind::Sound => &t.sounds,
        Kind::Runner => &t.play,
        _ => &t.shaders,
    };
    let shader = if names.is_empty() {
        String::new()
    } else {
        let r = rng.random();
        let index = ((r * names.len() as f32) as usize).min(names.len() - 1);
        names[index].clone()
    };
    let seed = (rng.random() * 1265536.0) as i32;

    let flags = t.flags;
    size[2] = parm(flags, Channel::Size, size[2], start_ms, life);
    alpha[2] = parm(flags, Channel::Alpha, alpha[2], start_ms, life);
    length[2] = parm(flags, Channel::Length, length[2], start_ms, life);
    size2[2] = parm(flags, Channel::Size2, size2[2], start_ms, life);
    let rgb_parm = parm(flags, Channel::Rgb, rgb_parm, start_ms, life);

    let physics = flags.contains(Flags::USE_PHYSICS) || flags.contains(Flags::EXPENSIVE_PHYSICS);
    let path = (t.kind.moving() && physics).then(|| Box::new(Path::new(point, velocity)));
    Spawned {
        kind: t.kind,
        start_ms,
        end_ms: start_ms + life,
        origin: point,
        origin2,
        velocity,
        acceleration,
        size,
        alpha,
        rgb_start,
        rgb_end,
        rgb_parm,
        normal: axis[0],
        length,
        size2,
        rotation,
        rotation_delta,
        chaos,
        path,
        shader,
        flags,
        seed,
    }
}

/// A sprite at `origin` (not from a file): it grows from `size_start` to
/// `size_end` and fades from `alpha_start` to `alpha_end` as `flags` say,
/// over `life_ms` from `now_ms`.
pub fn sprite_particle(
    origin: Vec3,
    (size_start, size_end): (f32, f32),
    (alpha_start, alpha_end): (f32, f32),
    now_ms: f32,
    life_ms: f32,
    shader: &str,
    flags: Flags,
) -> Spawned {
    Spawned {
        kind: Kind::Particle,
        start_ms: now_ms,
        end_ms: now_ms + life_ms.trunc(),
        origin,
        origin2: Vec3::ZERO,
        velocity: Vec3::ZERO,
        acceleration: Vec3::ZERO,
        size: [size_start, size_end, 0.0],
        alpha: [alpha_start, alpha_end, 0.0],
        rgb_start: Vec3::ONE,
        rgb_end: Vec3::ONE,
        rgb_parm: 0.0,
        normal: Vec3::X,
        length: [0.0; 3],
        size2: [0.0; 3],
        rotation: 0.0,
        rotation_delta: 0.0,
        chaos: 0.0,
        path: None,
        shader: shader.to_string(),
        flags,
        seed: 0,
    }
}

/// A drifting puff of smoke: it grows from 8 to `radius + 8` and darkens
/// from `colour` to black over its life.
pub fn puff(
    origin: Vec3,
    velocity: Vec3,
    (radius, rotation): (f32, f32),
    colour: [u8; 3],
    now_ms: f32,
    life_ms: f32,
    shader: &str,
) -> Spawned {
    let flags = Flags::member(Channel::Size, LINEAR) | Flags::member(Channel::Rgb, LINEAR);
    let mut spawned = sprite_particle(
        origin,
        (8.0, radius + 8.0),
        (1.0, 1.0),
        now_ms,
        life_ms,
        shader,
        flags,
    );
    spawned.velocity = velocity;
    spawned.rgb_start = Vec3::from(colour.map(|c| f32::from(c) / 255.0));
    spawned.rgb_end = Vec3::ZERO;
    spawned.rotation = rotation;
    spawned
}

/// A line from `start` to `end`, its width and alpha going straight from
/// their starts to their ends over its life.
pub fn added_line(
    (start, end): (Vec3, Vec3),
    (size_start, size_end): (f32, f32),
    (alpha_start, alpha_end): (f32, f32),
    now_ms: f32,
    life_ms: f32,
    shader: &str,
) -> Spawned {
    let flags = Flags::member(Channel::Size, LINEAR) | Flags::member(Channel::Alpha, LINEAR);
    let mut spawned = sprite_particle(
        start,
        (size_start, size_end),
        (alpha_start, alpha_end),
        now_ms,
        life_ms,
        shader,
        flags,
    );
    spawned.kind = Kind::Line;
    spawned.origin2 = end;
    spawned
}

// --- In a frame ------------------------------------------------------------

/// A primitive as drawn in a frame: where, how big (a sprite's half width,
/// a bolt's radius), its colour and alpha bytes, and a sprite's turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub origin: Vec3,
    pub radius: f32,
    /// A tail's or cylinder's length, a cylinder's far size.
    pub length: f32,
    pub size2: f32,
    pub rgba: [u8; 4],
    pub rotation: f32,
}

impl Spawned {
    /// Within its life at `now_ms` (both ends included).
    pub fn alive(&self, now_ms: f32) -> bool {
        now_ms >= self.start_ms && now_ms <= self.end_ms
    }

    /// How much of a channel's start value shows at `now_ms` (the rest is
    /// its end value), before any random pick.
    fn weight(&self, channel: Channel, parm: f32, now_ms: f32) -> f32 {
        let (start, end) = (self.start_ms, self.end_ms);
        let linear = self.flags.has(channel, LINEAR);
        let base = if linear {
            1.0 - (now_ms - start) / (end - start).max(1.0)
        } else {
            1.0
        };
        let blend = |other: f32| {
            if linear {
                base * 0.5 + other * 0.5
            } else {
                other
            }
        };
        match self.flags.shape(channel) {
            Shape::None => base,
            Shape::NonLinear => {
                let late = if now_ms > parm {
                    1.0 - (now_ms - parm) / (end - parm)
                } else {
                    1.0
                };
                blend(late)
            }
            Shape::Wave => base * ((now_ms - start) * parm).cos(),
            Shape::Clamp => {
                let early = if now_ms < parm {
                    (parm - now_ms) / (parm - start)
                } else {
                    0.0
                };
                blend(early)
            }
        }
    }

    /// A channel's weight, picked at random below it when the channel says
    /// so.
    fn picked_weight(&self, channel: Channel, parm: f32, now_ms: f32, rng: &mut Rng) -> f32 {
        let w = self.weight(channel, parm, now_ms);
        if self.flags.has(channel, RANDOM) {
            rng.range(0.0, w)
        } else {
            w
        }
    }

    /// How it looks at `now_ms`, client frames `frame_ms` apart; `rng`
    /// makes the random channels' picks.
    pub fn shown(&self, now_ms: f32, frame_ms: f32, rng: &mut Rng) -> Shown {
        let frames = ((now_ms - self.start_ms) / frame_ms).round().max(0.0);
        let origin = self.moved(frames, frame_ms);
        let mix = |values: [f32; 3], w: f32| values[0] * w + values[1] * (1.0 - w);

        let w = self.picked_weight(Channel::Length, self.length[2], now_ms, rng);
        let length = mix(self.length, w);
        let w = self.picked_weight(Channel::Size2, self.size2[2], now_ms, rng);
        let size2 = mix(self.size2, w);
        let w = self.picked_weight(Channel::Size, self.size[2], now_ms, rng);
        let radius = mix(self.size, w);
        let w = self.picked_weight(Channel::Rgb, self.rgb_parm, now_ms, rng);
        let colour = self.rgb_start * w + self.rgb_end * (1.0 - w);
        let byte = |c: f32| ((c * 255.0) as i32).clamp(0, 255) as u8;
        let mut rgba = [byte(colour.x), byte(colour.y), byte(colour.z), 0];

        let w = self.weight(Channel::Alpha, self.alpha[2], now_ms);
        let mut alpha = mix(self.alpha, w).clamp(0.0, 1.0);
        if self.flags.has(Channel::Alpha, RANDOM) {
            alpha = rng.range(0.0, alpha);
        }
        let alpha = (alpha * 255.0).clamp(0.0, 255.0) as u32;
        if self.flags.contains(Flags::USE_ALPHA) {
            rgba[3] = alpha as u8;
        } else {
            for c in &mut rgba[..3] {
                *c = ((u32::from(*c) * alpha) >> 8) as u8;
            }
        }

        // The turn slows a little each frame; summed frame by frame, as the
        // game does, so the bits match.
        let (mut rotation, mut delta) = (self.rotation, self.rotation_delta);
        for _ in 0..frames as usize {
            rotation += (frame_ms * 0.01) * delta;
            delta *= 1.0 - (frame_ms * 0.0007);
        }

        Shown {
            origin,
            radius,
            length,
            size2,
            rgba,
            rotation,
        }
    }

    /// Where it is after `frames` client frames `frame_ms` apart: along its
    /// path's leg then, if it has one.
    fn moved(&self, frames: f32, frame_ms: f32) -> Vec3 {
        let leg = self.path.as_ref().and_then(|path| path.leg(frames));
        match leg {
            Some(leg) if leg.resting => leg.origin,
            Some(leg) => flight(
                leg.origin,
                leg.velocity,
                self.acceleration,
                frames - leg.frame,
                frame_ms,
            ),
            None => flight(
                self.origin,
                self.velocity,
                self.acceleration,
                frames,
                frame_ms,
            ),
        }
    }

    /// Traces a physics particle's way on to the client frame at `now_ms`,
    /// frames `frame_ms` apart: each frame's move one line through the
    /// level (`trace`: where a line from the first point to the second
    /// meets it, and the face's normal; `None` when nothing is in the way
    /// or it starts inside). One that meets the level bounces off it, its
    /// velocity mirrored in the face and scaled by its bounce; slow enough
    /// on a floor, it rests there. How many traces it made; none for a
    /// particle without physics, or at rest.
    pub fn collide(
        &mut self,
        now_ms: f32,
        frame_ms: f32,
        trace: &dyn Fn(Vec3, Vec3) -> Option<(Vec3, Vec3)>,
    ) -> usize {
        let (acceleration, bounce) = (self.acceleration, self.chaos);
        let due = ((now_ms.min(self.end_ms) - self.start_ms) / frame_ms)
            .round()
            .max(0.0);
        let Some(path) = self.path.as_mut() else {
            return 0;
        };
        let mut traces = 0;
        while path.traced < due {
            let leg = *path.legs.last().expect("a path starts with a leg");
            if leg.resting {
                path.traced = due;
                break;
            }
            let step =
                |frames: f32| flight(leg.origin, leg.velocity, acceleration, frames, frame_ms);
            let into = path.traced + 1.0 - leg.frame;
            let (from, to) = (step(into - 1.0), step(into));
            traces += 1;
            path.traced += 1.0;
            let Some((at, normal)) = trace(from, to) else {
                continue;
            };
            let velocity = leg.velocity + acceleration * (frame_ms / 1000.0) * into;
            let bounced = (velocity - normal * (2.0 * velocity.dot(normal))) * bounce;
            let resting = normal.z > FLOOR_NORMAL_Z && bounced.dot(normal) < REST_SPEED;
            path.legs.push(Leg {
                frame: path.traced,
                origin: at + normal * SURFACE_GAP,
                velocity: if resting { Vec3::ZERO } else { bounced },
                resting,
            });
        }
        traces
    }
}

/// Where a particle is after `frames` frames `frame_ms` apart, its velocity
/// taking each frame's acceleration before it moves: the sum of the
/// frames' steps, in closed form.
fn flight(origin: Vec3, velocity: Vec3, acceleration: Vec3, frames: f32, frame_ms: f32) -> Vec3 {
    let dt = frame_ms / 1000.0;
    (origin + ((velocity * frames) * dt))
        + (((((acceleration * dt) * dt) * frames) * (frames + 1.0)) / 2.0)
}

/// A face this upright or flatter is a floor a particle may rest on (its
/// normal's Z, JA's walkable slope).
const FLOOR_NORMAL_Z: f32 = collision::player::WALKABLE_NORMAL_Z;
/// Bounced off a floor slower than this away from it (units a second), a
/// particle rests: a hop under a tenth of a unit high.
const REST_SPEED: f32 = 10.0;
/// How far off a face a particle that met it is put (units), so its next
/// move starts clear of it.
const SURFACE_GAP: f32 = 0.125;

/// A physics particle's way (`Spawned::collide`): legs of free flight, each
/// from the frame it met the level, and how many frames are traced.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    legs: Vec<Leg>,
    traced: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Leg {
    /// The client frame it starts on, since the particle's start.
    frame: f32,
    origin: Vec3,
    /// Its velocity there, before the next frame's acceleration.
    velocity: Vec3,
    /// At rest on a floor: it stays at its origin.
    resting: bool,
}

impl Path {
    fn new(origin: Vec3, velocity: Vec3) -> Path {
        Path {
            legs: vec![Leg {
                frame: 0.0,
                origin,
                velocity,
                resting: false,
            }],
            traced: 0.0,
        }
    }

    /// The leg the particle is on after `frames`, within what is traced.
    fn leg(&self, frames: f32) -> Option<&Leg> {
        self.legs.iter().rev().find(|leg| leg.frame <= frames)
    }
}

// --- Geometry --------------------------------------------------------------

/// Four corners with texture coordinates, in triangle-strip order.
pub type Strip = [(Vec3, [f32; 2]); 4];

/// Two directions square to `forward` and to each other: the first from
/// `forward` with its components turned one place (x from z, y from minus
/// x, z from y) and made square to it, the second their cross product.
/// Any vector works; a zero one gives zeros.
fn square_pair(forward: Vec3) -> (Vec3, Vec3) {
    let turned = Vec3::new(forward.z, -forward.x, forward.y);
    let right = (turned - forward * turned.dot(forward)).normalize_or_zero();
    (right, right.cross(forward))
}

/// A unit vector square to `v`, from the world axis `v` is least along.
fn perpendicular(v: Vec3) -> Vec3 {
    let size = v.abs();
    let least = if size.x <= size.y && size.x <= size.z {
        Vec3::X
    } else if size.y <= size.z {
        Vec3::Y
    } else {
        Vec3::Z
    };
    (least - v * least.dot(v)).normalize_or(Vec3::Y)
}

/// A band from `start` to `end`, `side` across it, half `w0` wide at the
/// start and `w1` at the end.
fn strip(start: Vec3, end: Vec3, side: Vec3, w0: f32, w1: f32) -> Strip {
    [
        (start + side * w0, [0.0, 0.0]),
        (start - side * w0, [1.0, 0.0]),
        (end + side * w1, [0.0, 1.0]),
        (end - side * w1, [1.0, 1.0]),
    ]
}

/// The way across the segment `a → b` that faces `eye`.
fn facing(a: Vec3, b: Vec3, eye: Vec3) -> Vec3 {
    (a - eye).cross(b - eye).normalize_or_zero()
}

/// A square of half width `radius` at `origin` spanned by `left` and `up`,
/// turned by `rotation` degrees.
pub fn sprite(
    origin: Vec3,
    radius: f32,
    rotation: f32,
    left: Vec3,
    up: Vec3,
) -> [(Vec3, [f32; 2]); 4] {
    let (l, u) = if rotation == 0.0 {
        (left * radius, up * radius)
    } else {
        let (s, c) = rotation.to_radians().sin_cos();
        (
            left * (c * radius) - up * (s * radius),
            up * (c * radius) + left * (s * radius),
        )
    };
    [
        (origin + l + u, [0.0, 0.0]),
        (origin - l + u, [1.0, 0.0]),
        (origin - l - u, [1.0, 1.0]),
        (origin + l - u, [0.0, 1.0]),
    ]
}

/// An axis with `forward` first and two directions square to it.
pub fn axis(forward: Vec3) -> [Vec3; 3] {
    let (right, second) = square_pair(forward);
    [forward, right, second]
}

/// A line's band, `radius` wide either side, facing `eye`.
pub fn line(spawned: &Spawned, radius: f32, eye: Vec3) -> Strip {
    let (a, b) = (spawned.origin, spawned.origin2);
    strip(a, b, facing(a, b, eye), radius, radius)
}

/// A tail's band: from where it is back toward where it was a frame ago,
/// `shown.length` long.
pub fn tail(spawned: &Spawned, shown: &Shown, (now_ms, frame_ms): (f32, f32), eye: Vec3) -> Strip {
    let frames = ((now_ms - spawned.start_ms) / frame_ms).round().max(0.0);
    let before = spawned.moved((frames - 1.0).max(0.0), frame_ms);
    let start = shown.origin;
    let end = start + (before - start).normalize_or_zero() * shown.length;
    strip(
        start,
        end,
        facing(start, end, eye),
        shown.radius,
        shown.radius,
    )
}

/// An oriented particle's square, facing along its normal.
pub fn oriented(spawned: &Spawned, shown: &Shown) -> [(Vec3, [f32; 2]); 4] {
    let (right, second) = square_pair(spawned.normal.normalize_or(Vec3::X));
    sprite(shown.origin, shown.radius, shown.rotation, right, second)
}

/// A cylinder's sides, one band per segment: `size2` round at its origin,
/// `radius` round `length` along its normal. Nearer the eye, more segments
/// (8 to 32).
pub fn cylinder(spawned: &Spawned, shown: &Shown, eye: Vec3) -> Vec<Strip> {
    let along = spawned.normal.normalize_or(Vec3::X);
    let near = shown.origin;
    let far = near + along * shown.length;
    let distance = ((near + far) / 2.0 - eye).length();
    let segments = (32.0 * (1.0 - distance / 1024.0)).clamp(8.0, 32.0) as usize;
    let up = square_pair(along).1;
    let ring = |centre: Vec3, radius: f32, i: usize| {
        let angle = ((360.0 / segments as f32) * i as f32).to_radians();
        centre + Quat::from_axis_angle(along, angle) * (up * radius)
    };
    let step = 1.0 / segments as f32;
    (0..segments)
        .map(|i| {
            let j = (i + 1) % segments;
            let (s0, s1) = (step * i as f32, step * (i + 1) as f32);
            [
                (ring(near, shown.size2, i), [s0, 1.0]),
                (ring(far, shown.radius, i), [s0, 0.0]),
                (ring(far, shown.radius, j), [s1, 0.0]),
                (ring(near, shown.size2, j), [s1, 1.0]),
            ]
        })
        .collect()
}

/// A decal's square, laid just off the surface square to its normal and
/// turned by its rotation, its start size round.
pub fn mark(spawned: &Spawned) -> Strip {
    let normal = spawned.normal.normalize_or(Vec3::Z);
    let across = perpendicular(normal);
    let second = Quat::from_axis_angle(normal, spawned.rotation.to_radians()) * across;
    let first = normal.cross(second);
    let r = spawned.size[0];
    let centre = spawned.origin + normal * 0.1;
    [
        (centre - first * r - second * r, [0.0, 0.0]),
        (centre + first * r - second * r, [1.0, 0.0]),
        (centre + first * r + second * r, [1.0, 1.0]),
        (centre - first * r + second * r, [0.0, 1.0]),
    ]
}

/// How far apart a bolt's points are (units).
const BOLT_STEP: u32 = 20;
/// The chance a point forks is the path generator's number above this.
const FORK_ABOVE: f32 = 0.94;
/// How many side bolts a branching bolt may fork, all told.
const FORKS: u32 = 3;

/// What one bolt's paths share while it is drawn.
struct Bolt<'a> {
    /// The way across every band, facing the eye.
    side: Vec3,
    taper: bool,
    chaos: f32,
    /// Forks left.
    forks: u32,
    /// Where the main path ends; forks head halfway there.
    main_end: Vec3,
    /// Picks the points and forks: seeded by the bolt, the same each frame.
    path: Rng,
    /// Picks the kinks: new each frame.
    shape: &'a mut Rng,
    strips: Vec<Strip>,
}

impl Bolt<'_> {
    /// Draws a jagged path from `a` to `b`, `width` wide, forking off side
    /// paths as it goes.
    fn draw(&mut self, a: Vec3, b: Vec3, width: f32) {
        let delta = b - a;
        let length = delta.length();
        let way = delta.normalize_or_zero();
        let (right, up) = square_pair(way);
        let taper = self.taper;
        let width_at = |x: f32| {
            if taper { width * (1.0 - x * x) } else { width }
        };
        let mut drift = Vec3::splat(10.0);
        let (mut previous, mut previous_x) = (a, 0.0);
        let mut k = BOLT_STEP;
        while k as f32 <= length {
            let x = if (k + BOLT_STEP) as f32 > length {
                1.0
            } else {
                k as f32 / length
            };
            let c1 = self.path.signed_unit();
            let c2 = self.path.signed_unit();
            let c3 = self.path.signed_unit();
            drift += way * (c1 * 3.0)
                + right * ((c2 * 7.0) * self.chaos)
                + up * ((c3 * 7.0) * self.chaos);
            let point = (a + drift) * (1.0 - x) + b * x;
            self.kinked(point, previous, width_at(x), width_at(previous_x));
            if self.forks > 0 {
                let r = self.path.random();
                if r > FORK_ABOVE && width * (1.0 - x) > 0.2 {
                    self.forks -= 1;
                    let mut toward = (point + self.main_end) * 0.5;
                    toward.x += self.path.signed_unit() * 80.0;
                    toward.y += self.path.signed_unit() * 80.0;
                    toward.z += self.path.signed_unit() * 80.0;
                    self.draw(point, toward, width_at(x));
                }
            }
            previous = point;
            previous_x = x;
            k += BOLT_STEP;
        }
    }

    /// One step of a path, from `s` (`ws` wide) to `e` (`we` wide), drawn
    /// as three bands through two kinks off the straight line.
    fn kinked(&mut self, s: Vec3, e: Vec3, ws: f32, we: f32) {
        let k: [f32; 6] = std::array::from_fn(|_| self.shape.signed_unit());
        let (aw, ar, au) = (0.66 + k[0] * 0.1, 0.07 + k[1] * 0.025, 0.07 + k[2] * 0.025);
        let (bw, br, bu) = (0.33 + k[3] * 0.1, (-ar) + k[4] * 0.02, (-au) + k[5] * 0.02);
        let span = e - s;
        let reach = span.length() * 0.7;
        let (right, up) = square_pair(span.normalize_or_zero());
        let kink =
            |w: f32, r: f32, u: f32| s * w + e * (1.0 - w) + right * (reach * r) + up * (reach * u);
        let (first, second) = (kink(aw, ar, au), kink(bw, br, bu));
        let w1 = ws * 0.666 + we * 0.333;
        let w2 = ws * 0.333 + we * 0.666;
        let side = self.side;
        self.strips.push(strip(s, first, side, ws, w1));
        self.strips.push(strip(second, first, side, w1, w2));
        self.strips.push(strip(second, e, side, w2, we));
    }
}

/// A lightning bolt's bands at `now_ms`, `radius` wide, facing `eye`. Its
/// path comes from its own seed, so it holds for its life; `shape` jitters
/// the kinks each frame. A tapering bolt narrows toward its end; a growing
/// one reaches its end over its life; a branching one forks up to three
/// side bolts in all (single player's budget; multiplayer's renderer never
/// fills it, QUIRKS.md Q-0015, which this follows the fix of).
pub fn bolt(spawned: &Spawned, radius: f32, now_ms: f32, eye: Vec3, shape: &mut Rng) -> Vec<Strip> {
    let start = spawned.origin;
    let mut end = spawned.origin2;
    let delta = end - start;
    let (way, length) = (delta.normalize_or_zero(), delta.length());
    if spawned.flags.contains(Flags::USE_BBOX) {
        let life = (spawned.end_ms - spawned.start_ms).max(1.0);
        let done = (1.0 - (spawned.end_ms - now_ms) / life).clamp(0.0, 1.0);
        end = start + way * (done * length);
    }
    let mut bolt = Bolt {
        side: facing(start, end, eye),
        taper: spawned.flags.contains(Flags::USE_MODEL),
        chaos: spawned.chaos,
        forks: if spawned.flags.contains(Flags::USE_PHYSICS) {
            FORKS
        } else {
            0
        },
        main_end: end,
        path: Rng(spawned.seed),
        shape,
        strips: Vec::new(),
    };
    bolt.draw(start, end, radius);
    bolt.strips
}
