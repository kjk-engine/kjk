//! A mesh written anew each frame is freed and allocated again on the GPU
//! each frame, so these quads are placed by a list written in place. The
//! list's unused quads are all at the origin, where none is drawn.

use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;

use crate::material::{OdysseyMaterial, PARTICLE_BYTES};
use crate::model_fx::SprayAssets;

const FIRST_QUADS: usize = 16;

pub(crate) struct QuadList {
    mesh: Handle<Mesh>,
    list: Handle<ShaderBuffer>,
    material: Handle<OdysseyMaterial>,
    quads: usize,
    coloured: bool,
    white: Vec<[f32; 4]>,
}

#[derive(Default)]
pub(crate) struct Quads {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colours: Option<Vec<[f32; 4]>>,
}

impl QuadList {
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
            white: Vec::new(),
        }
    }

    pub fn drawn(&self) -> (Handle<Mesh>, Handle<OdysseyMaterial>) {
        (self.mesh.clone(), self.material.clone())
    }

    pub fn write(&mut self, quads: &Quads, assets: SprayAssets) {
        let (meshes, buffers, materials) = assets;
        let vertices = quads.positions.len();
        let grown = vertices / 4 > self.quads;
        if grown {
            self.quads = (vertices / 4).next_power_of_two();
            let _ = meshes.insert(&self.mesh, blank(self.quads, self.coloured));
        }
        let colours = match &quads.colours {
            Some(colours) => colours.as_slice(),
            None => {
                if self.white.len() < vertices {
                    self.white.resize(vertices, [1.0; 4]);
                }
                &self.white[..vertices]
            }
        };
        let bytes = crate::model_fx::list_bytes(
            Vec::new(),
            &quads.positions,
            &quads.uvs,
            colours,
            self.quads,
        );
        if grown {
            self.list = buffers.add(crate::model_fx::particle_list(bytes));
            if let Some(mut material) = materials.get_mut(&self.material) {
                material.particles = self.list.clone();
            }
        } else if let Some(mut list) = buffers.get_mut(&self.list) {
            list.data = Some(bytes);
        }
    }
}

fn blank(quads: usize, coloured: bool) -> Mesh {
    if coloured {
        return crate::model_fx::blank_quads(quads);
    }
    let vertices = quads * 4;
    crate::scene::blade_mesh(vec![[0.0; 3]; vertices], vec![[0.0; 2]; vertices])
}
