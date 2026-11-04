//! The game's icons as textures, loaded the first time a row shows one and
//! kept by name; a missing icon is a blank square of the same size.

use std::collections::HashMap;

use crate::ui::style::ROW;
use save_editor::model::catalog::Catalog;

#[derive(Default)]
pub struct Icons {
    loaded: HashMap<(vfs::Game, String), Option<egui::TextureHandle>>,
}

impl Icons {
    /// Icon `name` of `catalog`'s install at `size` pixels square, or a
    /// blank square.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&Catalog>,
        name: Option<&str>,
        size: f32,
    ) {
        let texture = match (catalog, name) {
            (Some(catalog), Some(name)) => self
                .loaded
                .entry((catalog.game, name.to_string()))
                .or_insert_with(|| {
                    let icon = catalog.icon(name)?;
                    let image = egui::ColorImage::from_rgba_unmultiplied(icon.size, &icon.rgba);
                    Some(
                        ui.ctx()
                            .load_texture(name, image, egui::TextureOptions::LINEAR),
                    )
                })
                .clone(),
            _ => None,
        };
        let size = egui::vec2(size, size);
        match texture {
            Some(texture) => {
                ui.add(egui::Image::from_texture((texture.id(), size)));
            }
            None => {
                ui.allocate_exact_size(size, egui::Sense::hover());
            }
        }
    }

    /// Icon `name` at row height.
    pub fn row(&mut self, ui: &mut egui::Ui, catalog: Option<&Catalog>, name: Option<&str>) {
        self.show(ui, catalog, name, ROW);
    }
}
