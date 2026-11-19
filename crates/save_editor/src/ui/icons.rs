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
        let size = egui::vec2(size, size);
        match self.texture(ui, catalog, name) {
            Some(texture) => {
                ui.add(egui::Image::from_texture((texture.id(), size)));
            }
            None => {
                ui.allocate_exact_size(size, egui::Sense::hover());
            }
        }
    }

    /// Icon `name` as a button `size` pixels square, framed when
    /// `selected`; its name instead when the install lacks it.
    pub fn button(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&Catalog>,
        name: &str,
        (size, selected): (f32, bool),
    ) -> egui::Response {
        let button = match self.texture(ui, catalog, Some(name)) {
            Some(texture) => egui::Button::image(egui::Image::from_texture((
                texture.id(),
                egui::vec2(size, size),
            ))),
            None => egui::Button::new(name),
        };
        ui.add(button.selected(selected))
    }

    /// Icon `name`'s texture, loaded the first time it is asked for.
    fn texture(
        &mut self,
        ui: &egui::Ui,
        catalog: Option<&Catalog>,
        name: Option<&str>,
    ) -> Option<egui::TextureHandle> {
        let (catalog, name) = (catalog?, name?);
        self.loaded
            .entry((catalog.game, name.to_string()))
            .or_insert_with(|| {
                let icon = catalog.icon(name)?;
                let image = egui::ColorImage::from_rgba_unmultiplied(icon.size, &icon.rgba);
                Some(
                    ui.ctx()
                        .load_texture(name, image, egui::TextureOptions::LINEAR),
                )
            })
            .clone()
    }

    /// Icon `name` at row height.
    pub fn row(&mut self, ui: &mut egui::Ui, catalog: Option<&Catalog>, name: Option<&str>) {
        self.show(ui, catalog, name, ROW);
    }
}
