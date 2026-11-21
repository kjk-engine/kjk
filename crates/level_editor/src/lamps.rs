//! The asset picker's Lamps tab: the lamps of the install that can be
//! placed by hand (`level_edit::lamps`, `kjk_game::lamps`).
//!
//! The library looks through two sets of models, a few each frame while
//! the tab is shown, so the window keeps drawing:
//! - **Every placeable model** that `placeables.2da` names. A model is a
//!   lamp when one of its meshes glows with a texture that the light data
//!   calls a lamp on placeables. It is placed whole.
//! - **Every room model** of the install's layouts. Each mesh that glows
//!   with a texture the light data calls a lamp on rooms is a light mesh,
//!   placed alone. A light mesh of the same texture and about the same
//!   size as one listed already is left out, so a room's row of equal
//!   panels is listed once. "About the same size" is each of its three
//!   extents, longest first, the same to the nearest half metre. At most
//!   `PER_TEXTURE` light meshes of one texture are listed.
//!
//! "Glows" is as the fitted lights have it: the mesh's self-illumination is
//! above 0, or its kind shines where its mesh does not glow (`always`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use vfs::resolver::Resolver;

/// A lamp the picker offers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LampAsset {
    /// The model's name.
    pub model: String,
    /// The node of the light mesh; empty for a whole placeable model.
    pub mesh: String,
    /// The glowing texture that makes it a lamp.
    pub texture: String,
}

impl LampAsset {
    /// The name the picker shows: the texture, then where it is from.
    pub fn label(&self) -> String {
        match self.mesh.is_empty() {
            true => format!("{} (placeable {})", self.texture, self.model),
            false => format!("{} (room {})", self.texture, self.model),
        }
    }

    /// The name its thumbnail is kept under: the model, and its mesh after
    /// a `|`.
    pub fn key(&self) -> String {
        match self.mesh.is_empty() {
            true => self.model.clone(),
            false => format!("{}|{}", self.model, self.mesh),
        }
    }

    /// The lamp a thumbnail's name (`key`) names.
    pub fn of_key(key: &str) -> (String, String) {
        match key.split_once('|') {
            Some((model, mesh)) => (model.to_string(), mesh.to_string()),
            None => (key.to_string(), String::new()),
        }
    }
}

/// The lamps found so far, and the models still to look through.
#[derive(Default)]
pub struct LampLibrary {
    /// Each model to look at, and whether it is a placeable's.
    to_look: VecDeque<(String, bool)>,
    total: usize,
    started: bool,
    /// The lamps found, in the order of their labels.
    pub found: Vec<LampAsset>,
    /// The light meshes listed: their texture and size (half metres).
    seen: BTreeSet<(String, [i32; 3])>,
    /// How many light meshes of each texture are listed.
    per_texture: BTreeMap<String, usize>,
}

/// The most light meshes of one texture the library lists.
const PER_TEXTURE: usize = 4;

/// How long a frame spends looking through models, milliseconds.
const BUDGET_MS: f64 = 6.0;

impl LampLibrary {
    /// Starts the look through the placeables' models and `rooms`, once.
    pub fn start(&mut self, resolver: &Resolver, rooms: impl Iterator<Item = String>) {
        if self.started {
            return;
        }
        self.started = true;
        let mut placeables: Vec<String> = Vec::new();
        if let Some(table) = resolver.two_da("placeables") {
            for row in 0..table.cells.len() {
                placeables.extend(table.resref(row, "modelname"));
            }
        }
        placeables.sort();
        placeables.dedup();
        self.to_look
            .extend(placeables.into_iter().map(|model| (model, true)));
        self.to_look.extend(rooms.map(|model| (model, false)));
        self.total = self.to_look.len();
    }

    /// How many models are looked through, of how many, while the look
    /// goes on.
    pub fn progress(&self) -> Option<(usize, usize)> {
        (self.started && !self.to_look.is_empty())
            .then(|| (self.total - self.to_look.len(), self.total))
    }

    /// Looks through models for a frame's time (`BUDGET_MS`).
    pub fn step(&mut self, resolver: &Resolver, game: &str) {
        let started = std::time::Instant::now();
        while started.elapsed().as_secs_f64() * 1000.0 < BUDGET_MS {
            let Some((model, placeable)) = self.to_look.pop_front() else {
                break;
            };
            self.look(resolver, game, &model, placeable);
        }
    }

    /// Looks through every model at once (for `--cmds` and the tests).
    pub fn finish(&mut self, resolver: &Resolver, game: &str) {
        while let Some((model, placeable)) = self.to_look.pop_front() {
            self.look(resolver, game, &model, placeable);
        }
    }

    fn look(&mut self, resolver: &Resolver, game: &str, model: &str, placeable: bool) {
        let Ok(Some(ir)) = kjk_game::viewer::model(resolver, model) else {
            return;
        };
        for mesh in &ir.meshes {
            let Some(texture) = mesh.texture.as_deref() else {
                continue;
            };
            if mesh.skin.is_some() || mesh.positions.is_empty() {
                continue;
            }
            let Some(always) = kjk_game::fitted_lights::lamp_kind(game, texture, placeable) else {
                continue;
            };
            if !always && mesh.self_illum.iter().all(|&v| v <= 0.0) {
                continue;
            }
            if placeable {
                self.add(LampAsset {
                    model: model.to_string(),
                    mesh: String::new(),
                    texture: texture.to_string(),
                });
                // A whole model is listed once.
                return;
            }
            let Some(node) = ir.nodes.get(mesh.node) else {
                continue;
            };
            let (mut low, mut high) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
            for point in &mesh.positions {
                for axis in 0..3 {
                    low[axis] = low[axis].min(point[axis]);
                    high[axis] = high[axis].max(point[axis]);
                }
            }
            let mut extents: [f32; 3] = std::array::from_fn(|axis| high[axis] - low[axis]);
            extents.sort_by(|a, b| b.total_cmp(a));
            let size = extents.map(|extent| (extent * 2.0).round() as i32);
            let listed = self.per_texture.entry(texture.to_string()).or_insert(0);
            if *listed >= PER_TEXTURE || !self.seen.insert((texture.to_string(), size)) {
                continue;
            }
            *listed += 1;
            self.add(LampAsset {
                model: model.to_string(),
                mesh: node.name.clone(),
                texture: texture.to_string(),
            });
        }
    }

    fn add(&mut self, lamp: LampAsset) {
        let label = lamp.label();
        let at = self.found.partition_point(|found| found.label() < label);
        self.found.insert(at, lamp);
    }
}
