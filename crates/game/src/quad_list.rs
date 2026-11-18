//! Quads drawn each frame where they are now (a lightsaber blade's glow,
//! core and trail, Jedi Academy's effects), placed by a list the GPU reads
//! (`OdysseyMaterial::particles`), as an emitter's particles are
//! (`model_fx::Spray`).
//!
//! A mesh written anew each frame is freed and allocated again on the GPU
//! each frame. A list is written in place instead. The mesh is quads of
//! nothing, made once, and its vertices take their places, texture
//! coordinates and colours from the list by their index. The list's unused
//! quads are all at the origin, where none is drawn. The mesh and the list
//! grow together when the quads outgrow them.

use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;

use crate::material::{OdysseyMaterial, PARTICLE_BYTES};
use crate::model_fx::SprayAssets;

/// The quads a list and its mesh first hold.
const FIRST_QUADS: usize = 16;

/// A material's quads placed by a list of their own.
pub(crate) struct QuadList {
    mesh: Handle<Mesh>,
    list: Handle<ShaderBuffer>,
    material: Handle<OdysseyMaterial>,
    quads: usize,
    /// Its vertices carry colours.
    coloured: bool,
}

/// Quads' vertices, four a quad: places, texture coordinates and, for
/// quads with colours, their colours.
#[derive(Default)]
pub(crate) struct Quads {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colours: Option<Vec<[f32; 4]>>,
}

impl QuadList {
    /// `material` drawing quads placed by a list of their own. The quads'
    /// vertices carry colours when `coloured` is set.
    pub fn new(mut material: OdysseyMaterial, coloured: bool, assets: SprayAssets) -> QuadList {
        let (meshes, buffers, materials) = assets;
        let quads = FIRST_QUADS;
        let list = buffers.add(crate::model_fx::particle_list(vec![
            0;
            quads
                * 4
                * PARTICLE_BYTES
        ]));
        material.particles = list.clone();
        material.listed_quads = true;
        QuadList {
            mesh: meshes.add(blank(quads, coloured)),
            list,
            material: materials.add(material),
            quads,
            coloured,
        }
    }

    /// Its mesh and its material, to spawn.
    pub fn drawn(&self) -> (Handle<Mesh>, Handle<OdysseyMaterial>) {
        (self.mesh.clone(), self.material.clone())
    }

    /// Its quads now. Quads without colours are written white, which a
    /// mesh without colours never reads.
    pub fn write(&mut self, quads: &Quads, assets: SprayAssets) {
        let (meshes, buffers, materials) = assets;
        let count = quads.positions.len() / 4;
        if count > self.quads {
            self.quads = count.next_power_of_two();
            self.list = buffers.add(crate::model_fx::particle_list(Vec::new()));
            let _ = meshes.insert(&self.mesh, blank(self.quads, self.coloured));
            if let Some(mut material) = materials.get_mut(&self.material) {
                material.particles = self.list.clone();
            }
        }
        let white;
        let colours = match &quads.colours {
            Some(colours) => colours,
            None => {
                white = vec![[1.0; 4]; quads.positions.len()];
                &white
            }
        };
        let bytes = crate::model_fx::list_bytes(&quads.positions, &quads.uvs, colours, self.quads);
        if let Some(mut list) = buffers.get_mut(&self.list) {
            list.data = Some(bytes);
        }
    }
}

/// `quads` quads of nothing, with or without colours.
fn blank(quads: usize, coloured: bool) -> Mesh {
    if coloured {
        return crate::model_fx::blank_quads(quads);
    }
    let vertices = quads * 4;
    crate::scene::blade_mesh(vec![[0.0; 3]; vertices], vec![[0.0; 2]; vertices])
}
