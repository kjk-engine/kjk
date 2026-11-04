//! The compact look: small type, tight spacing, one line per row.

use egui::{FontFamily, FontId, TextStyle, vec2};

/// The body text's size; a row is this plus two pixels, and so is an icon.
pub const BODY: f32 = 12.0;
/// A list row's height, and an icon's size.
pub const ROW: f32 = BODY + 2.0;

/// The editor's style on every theme.
pub fn compact(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(14.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(BODY, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(BODY, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(10.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(11.0, FontFamily::Monospace),
            ),
        ]
        .into();
        let spacing = &mut style.spacing;
        spacing.item_spacing = vec2(4.0, 2.0);
        spacing.button_padding = vec2(4.0, 1.0);
        spacing.interact_size = vec2(32.0, ROW + 2.0);
        spacing.window_margin = egui::Margin::same(4);
        spacing.menu_margin = egui::Margin::same(4);
        spacing.indent = 10.0;
        spacing.combo_height = 300.0;
        spacing.scroll.bar_width = 6.0;
        style.interaction.tooltip_delay = 0.2;
    });
}
