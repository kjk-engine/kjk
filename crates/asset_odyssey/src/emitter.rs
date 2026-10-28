//! Emitter nodes: where a model's particles come from and how they live.
//! The header names the kind (update, render, blend, texture); the
//! particles' numbers are controllers, read at their first row as the rest
//! pose, which a clip's own rows replace (a pose clip such as the character
//! sheet's alignment backdrops, `charrec_light`'s `evil`, `align1`..`align19`,
//! `good`). Controller numbers: `mdl::emitter_controller`.

use fmt_odyssey::mdl;

/// How particles are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    /// A steady stream at the birth rate.
    Fountain,
    /// One particle, kept alive.
    Single,
    /// A burst when told.
    Explosion,
    Lightning,
    Other,
}

/// How a particle is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Render {
    /// A square facing the eye.
    Normal,
    /// A square in the emitter's plane, facing its Z.
    BillboardToLocalZ,
    /// A square lying flat, facing the world's Z.
    BillboardToWorldZ,
    /// Standing on the world's Z, turned to the eye about it.
    AlignedToWorldZ,
    /// Stretched along its flight, facing the eye about it.
    AlignedToParticleDir,
    /// Stretched back along its flight by its speed and blur length.
    MotionBlur,
    Linked,
}

/// How a particle's colour meets what is behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    /// Alpha-blended.
    Normal,
    /// Added, weighted by its alpha.
    Lighten,
    /// Opaque where its alpha is high.
    PunchThrough,
}

/// The particles' numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Particles a second.
    pub birthrate: f32,
    pub random_birthrate: f32,
    /// Seconds a particle lives; -1 forever.
    pub life: f32,
    /// Metres a second along the emitter's Z, plus up to `random_velocity`.
    pub velocity: f32,
    pub random_velocity: f32,
    /// The cone the particles leave in (radians across).
    pub spread: f32,
    /// Pulls a particle down (up when negative), metres a second squared.
    pub mass: f32,
    pub drag: f32,
    /// How fast each particle turns (radians a second).
    pub particle_rot: f32,
    /// The patch particles are born on, centimetres across (X, Y).
    pub area: [f32; 2],
    /// At birth, at `percent_mid` of its life, at death.
    pub colour: [[f32; 3]; 3],
    pub alpha: [f32; 3],
    /// Metres across; `size_y` its height when not 0 (else as wide).
    pub size: [f32; 3],
    pub size_y: [f32; 3],
    /// Where in a particle's life the middle values hold (0.5 in K1,
    /// which has no such controller).
    pub percent_mid: f32,
    /// The flipbook's first and last frame, and frames a second (0: the
    /// frames run once over a particle's life).
    pub frames: [f32; 2],
    pub fps: f32,
    pub blur_length: f32,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            birthrate: 0.0,
            random_birthrate: 0.0,
            life: 1.0,
            velocity: 0.0,
            random_velocity: 0.0,
            spread: 0.0,
            mass: 0.0,
            drag: 0.0,
            particle_rot: 0.0,
            area: [0.0; 2],
            colour: [[1.0; 3]; 3],
            alpha: [1.0; 3],
            size: [1.0; 3],
            size_y: [0.0; 3],
            percent_mid: 0.5,
            frames: [0.0; 2],
            fps: 0.0,
            blur_length: 0.0,
        }
    }
}

impl Params {
    /// Controller `kind`'s value `row` (its first columns); false when it
    /// is not one of the particles' numbers.
    pub fn set(&mut self, kind: u32, row: &[f32]) -> bool {
        use mdl::emitter_controller::*;
        let Some(&first) = row.first() else {
            return false;
        };
        let rgb = || (row.len() >= 3).then(|| [row[0], row[1], row[2]]);
        match kind {
            ALPHA_END => self.alpha[2] = first,
            ALPHA_START => self.alpha[0] = first,
            ALPHA_MID => self.alpha[1] = first,
            BIRTHRATE => self.birthrate = first,
            RANDOM_BIRTHRATE => self.random_birthrate = first,
            DRAG => self.drag = first,
            FPS => self.fps = first,
            FRAME_END => self.frames[1] = first,
            FRAME_START => self.frames[0] = first,
            LIFE_EXP => self.life = first,
            MASS => self.mass = first,
            PARTICLE_ROT => self.particle_rot = first,
            RAND_VEL => self.random_velocity = first,
            SIZE_START => self.size[0] = first,
            SIZE_MID => self.size[1] = first,
            SIZE_END => self.size[2] = first,
            SIZE_START_Y => self.size_y[0] = first,
            SIZE_MID_Y => self.size_y[1] = first,
            SIZE_END_Y => self.size_y[2] = first,
            SPREAD => self.spread = first,
            VELOCITY => self.velocity = first,
            X_SIZE => self.area[0] = first,
            Y_SIZE => self.area[1] = first,
            BLUR_LENGTH => self.blur_length = first,
            PERCENT_MID => self.percent_mid = first,
            COLOR_START | COLOR_MID | COLOR_END => {
                let Some(rgb) = rgb() else {
                    return false;
                };
                let at = match kind {
                    COLOR_START => 0,
                    COLOR_MID => 1,
                    _ => 2,
                };
                self.colour[at] = rgb;
            }
            _ => return false,
        }
        true
    }
}

/// An emitter node at rest.
#[derive(Debug, Clone)]
pub struct EmitterIr {
    /// Its node in the model's list.
    pub node: usize,
    pub name: String,
    pub update: Update,
    pub render: Render,
    pub blend: Blend,
    /// Lowercase; `None` when it names none.
    pub texture: Option<String>,
    /// The flipbook's columns and rows (at least 1 each).
    pub grid: [u32; 2],
    pub looped: bool,
    pub params: Params,
}

impl EmitterIr {
    pub fn from_node(node: usize, source: &mdl::Node, emitter: &mdl::Emitter) -> EmitterIr {
        let mut params = Params::default();
        for controller in &source.controllers {
            let columns = usize::from(controller.columns);
            if let Some(row) = controller.values.get(..columns) {
                params.set(controller.kind, row);
            }
        }
        let word = |text: &str| text.trim().to_ascii_lowercase();
        EmitterIr {
            node,
            name: source.name.to_ascii_lowercase(),
            update: match word(&emitter.update).as_str() {
                "fountain" => Update::Fountain,
                "single" => Update::Single,
                "explosion" => Update::Explosion,
                "lightning" => Update::Lightning,
                _ => Update::Other,
            },
            render: match word(&emitter.render).as_str() {
                "billboard_to_local_z" => Render::BillboardToLocalZ,
                "billboard_to_world_z" => Render::BillboardToWorldZ,
                "aligned_to_world_z" => Render::AlignedToWorldZ,
                "aligned_to_particle_dir" => Render::AlignedToParticleDir,
                "motion_blur" => Render::MotionBlur,
                "linked" => Render::Linked,
                _ => Render::Normal,
            },
            blend: match word(&emitter.blend).as_str() {
                "lighten" => Blend::Lighten,
                "punch-through" | "punchthrough" => Blend::PunchThrough,
                _ => Blend::Normal,
            },
            texture: Some(word(&emitter.texture)).filter(|name| !name.is_empty() && name != "null"),
            grid: emitter.grid.map(|n| n.max(1)),
            looped: emitter.looped,
            params,
        }
    }
}

/// Metres a second squared that a mass of 1 falls by (KOTOR's emitters
/// take `mass` as a share of gravity).
const GRAVITY: f32 = 9.8;

/// One particle, in the model's space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// Seconds lived.
    pub age: f32,
    /// Its turn now and how fast it turns (radians, radians a second).
    pub spin: f32,
    pub turning: f32,
}

/// How a particle looks now: its colour, alpha, width and height, and
/// flipbook frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub colour: [f32; 3],
    pub alpha: f32,
    pub size: [f32; 2],
    pub frame: u32,
}

/// `values` at `t` of a life: from the first to the middle by
/// `percent_mid`, then to the last.
fn through<const N: usize>(values: [[f32; N]; 3], percent_mid: f32, t: f32) -> [f32; N] {
    let mid = percent_mid.clamp(0.01, 0.99);
    let (from, to, f) = if t < mid {
        (values[0], values[1], t / mid)
    } else {
        (values[1], values[2], (t - mid) / (1.0 - mid))
    };
    std::array::from_fn(|i| from[i] + (to[i] - from[i]) * f.clamp(0.0, 1.0))
}

/// A particle's look at its age.
pub fn look(params: &Params, grid: [u32; 2], particle: &Particle) -> Look {
    let t = if params.life > 0.0 {
        (particle.age / params.life).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mid = params.percent_mid;
    let [width] = through(params.size.map(|v| [v]), mid, t);
    let [height] = through(params.size_y.map(|v| [v]), mid, t);
    let [alpha] = through(params.alpha.map(|v| [v]), mid, t);
    let frames = (grid[0].max(1) * grid[1].max(1)) as f32;
    let [first, last] = params.frames.map(|f| f.clamp(0.0, frames - 1.0));
    let span = (last - first).max(0.0);
    let frame = if params.fps > 0.0 {
        first + (particle.age * params.fps).floor() % (span + 1.0)
    } else {
        first + (t * span).round()
    };
    Look {
        colour: through(params.colour, mid, t),
        alpha: alpha.clamp(0.0, 1.0),
        size: [width, if height > 0.0 { height } else { width }],
        frame: frame as u32,
    }
}

/// An emitter's particles over time. Random numbers come from its own
/// seed, so a stream replays the same.
#[derive(Debug, Clone)]
pub struct Stream {
    pub particles: Vec<Particle>,
    /// Births owed (a fraction of one carried between steps).
    owed: f32,
    seed: u32,
}

impl Stream {
    pub fn new(seed: u32) -> Stream {
        Stream {
            particles: Vec::new(),
            owed: 0.0,
            seed: seed.max(1),
        }
    }

    /// 0 to 1 (xorshift).
    fn random(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// `dt` seconds on: the old age and move (gravity by `mass`, slowed by
    /// `drag`), the dead go, and a fountain gives birth at its rate on the
    /// emitter's patch (`to_model`: the emitter node's transform, row-major)
    /// along its Z within the spread.
    pub fn step(
        &mut self,
        params: &Params,
        update: Update,
        to_model: &crate::model::Matrix,
        dt: f32,
    ) {
        let fall = params.mass * GRAVITY * dt;
        let slow = (1.0 - params.drag * dt).clamp(0.0, 1.0);
        for particle in &mut self.particles {
            particle.age += dt;
            particle.velocity[2] -= fall;
            particle.velocity = particle.velocity.map(|v| v * slow);
            for (p, v) in particle.position.iter_mut().zip(particle.velocity) {
                *p += v * dt;
            }
            particle.spin += particle.turning * dt;
        }
        let life = params.life;
        self.particles
            .retain(|particle| life < 0.0 || particle.age < life);
        if update != Update::Fountain || life == 0.0 {
            self.owed = 0.0;
            return;
        }
        let rate = params.birthrate + params.random_birthrate * self.random();
        self.owed += rate.max(0.0) * dt;
        // A step longer than a life gives no more than a life's worth.
        let most = (rate * life.max(0.0)).ceil().max(1.0);
        self.owed = self.owed.min(most);
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            let born = self.born(params, to_model);
            self.particles.push(born);
        }
    }

    fn born(&mut self, params: &Params, m: &crate::model::Matrix) -> Particle {
        let local = [
            (self.random() - 0.5) * params.area[0] * 0.01,
            (self.random() - 0.5) * params.area[1] * 0.01,
            0.0,
        ];
        let tilt = (self.random() * params.spread / 2.0).min(std::f32::consts::PI);
        let around = self.random() * std::f32::consts::TAU;
        let direction = [
            tilt.sin() * around.cos(),
            tilt.sin() * around.sin(),
            tilt.cos(),
        ];
        let speed = params.velocity + params.random_velocity * self.random();
        let point = |v: [f32; 3], w: f32| -> [f32; 3] {
            std::array::from_fn(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2] + m[r][3] * w)
        };
        let heading = point(direction, 0.0);
        Particle {
            position: point(local, 1.0),
            velocity: heading.map(|v| v * speed),
            age: 0.0,
            spin: self.random() * std::f32::consts::TAU,
            turning: params.particle_rot,
        }
    }
}
