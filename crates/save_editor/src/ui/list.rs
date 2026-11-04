//! The one list every page uses: one line a row, an icon first, only the
//! rows in view drawn (so thousands scroll smoothly).

use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};

use crate::ui::icons::Icons;
use crate::ui::style::ROW;
use save_editor::model::catalog::Catalog;

/// A column: its title and width (none: what is left).
pub struct Col {
    pub title: &'static str,
    pub width: Option<f32>,
}

pub const fn col(title: &'static str, width: f32) -> Col {
    Col {
        title,
        width: Some(width),
    }
}

pub const fn rest(title: &'static str) -> Col {
    Col { title, width: None }
}

/// One row: its key, icon and cells (the first beside the icon).
#[derive(Default)]
pub struct Row {
    pub key: u64,
    pub icon: Option<String>,
    pub cells: Vec<String>,
    /// Shown greyed (not known, not usable).
    pub grey: bool,
    /// A warning, shown as ⚠ with this as its tooltip.
    pub warning: Option<String>,
    /// A small grey tag after the first cell ("mod", the resref).
    pub tag: Option<String>,
}

/// What a click did.
pub struct Clicked {
    pub key: u64,
    pub double: bool,
}

/// `count` rows made by `row_of` (only those in view are made), under
/// `columns`' titles; `selected` follows a click.
#[allow(clippy::too_many_arguments)]
pub fn rows(
    ui: &mut egui::Ui,
    id: &str,
    columns: &[Col],
    count: usize,
    row_of: impl Fn(usize) -> Row,
    icons: &mut Icons,
    catalog: Option<&Catalog>,
    selected: &mut Option<u64>,
) -> Option<Clicked> {
    let mut clicked = None;
    let mut table = TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .sense(egui::Sense::click())
        .auto_shrink([false, false])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
    for column in columns {
        table = table.column(match column.width {
            Some(width) => Column::initial(width)
                .at_least(24.0)
                .clip(true)
                .resizable(true),
            None => Column::remainder().at_least(40.0).clip(true),
        });
    }
    table
        .header(ROW + 2.0, |mut header| {
            for column in columns {
                header.col(|ui| {
                    ui.label(RichText::new(column.title).small().strong());
                });
            }
        })
        .body(|body| {
            body.rows(ROW + 1.0, count, |mut table_row| {
                let row = row_of(table_row.index());
                table_row.set_selected(*selected == Some(row.key));
                let text = |text: &str| {
                    let text = RichText::new(text);
                    if row.grey { text.weak() } else { text }
                };
                for (index, cell) in row.cells.iter().enumerate() {
                    table_row.col(|ui| {
                        if index == 0 {
                            icons.row(ui, catalog, row.icon.as_deref());
                            if let Some(warning) = &row.warning {
                                ui.label(
                                    RichText::new("!")
                                        .strong()
                                        .color(Color32::from_rgb(230, 170, 40)),
                                )
                                .on_hover_text(warning);
                            }
                        }
                        ui.label(text(cell));
                        if index == 0
                            && let Some(tag) = &row.tag
                        {
                            ui.label(RichText::new(tag).small().weak());
                        }
                    });
                }
                let response = table_row.response();
                if response.clicked() || response.double_clicked() {
                    *selected = Some(row.key);
                    clicked = Some(Clicked {
                        key: row.key,
                        double: response.double_clicked(),
                    });
                }
            });
        });
    clicked
}
