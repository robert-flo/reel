//! La franja de arriba: nombre, version, tema y ajustes.

use egui::{Align, Layout, Sense, Vec2};
use fastframe_fonts::Weight;

use crate::app::App;

use super::{text, Metrics};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;

    egui::Panel::top("top-bar")
        .exact_size(Metrics::TOP_BAR)
        .frame(
            egui::Frame::new()
                .fill(palette.panel)
                .inner_margin(egui::Margin::symmetric(Metrics::GUTTER as i8, 0)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(text("reel", 15.0, Weight::Bold, palette.text));
                ui.add_space(8.0);
                ui.label(text(
                    env!("CARGO_PKG_VERSION"),
                    11.0,
                    Weight::Regular,
                    palette.dim,
                ));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if link(ui, app.tr().settings, app).clicked() {
                        app.open_settings();
                    }
                });
            });
        });
}

fn link(ui: &mut egui::Ui, label: &str, app: &App) -> egui::Response {
    let palette = app.palette;
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), Weight::Regular.font_id(11.0), palette.dim);
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + Vec2::new(4.0, 0.0), Sense::click());
    let color = if response.hovered() {
        palette.text
    } else {
        palette.dim
    };
    ui.painter().galley(rect.left_top(), galley, color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
