//! La fila del enlace: campo, pegar y el boton que manda a la cola.
//!
//! Enter en el campo hace lo mismo que el boton, porque es lo que uno intenta
//! primero.

use egui::{Key, Vec2};
use fastframe_fonts::Weight;

use crate::app::App;

use super::widgets::text;
use super::Metrics;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let height = Metrics::URL_ROW;

    ui.horizontal(|ui| {
        let button_width = 102.0;
        let field_width =
            ui.available_width() - (button_width * 2.0) - (Metrics::GAP * 2.0);

        let field = egui::TextEdit::singleline(&mut app.url)
            .hint_text(text(
                "pega un enlace",
                13.0,
                Weight::Regular,
                palette.dim,
            ))
            .font(Weight::Regular.font_id(13.0))
            .margin(egui::Margin::symmetric(16, 0))
            .vertical_align(egui::Align::Center)
            .desired_width(field_width);

        let response = ui.add_sized(Vec2::new(field_width, height), field);
        let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));

        ui.add_space(Metrics::GAP);
        let paste = ui.add_sized(
            Vec2::new(button_width, height),
            egui::Button::new(text("pegar", 13.0, Weight::Regular, palette.dim)),
        );
        if paste.clicked() {
            if let Some(clipped) = app.clipboard_text(ui.ctx()) {
                app.url = clipped;
                response.request_focus();
            }
        }

        ui.add_space(Metrics::GAP);
        let enabled = !app.url.trim().is_empty();
        let go = ui.add_enabled(
            enabled,
            egui::Button::new(text("yoink", 14.0, Weight::SemiBold, palette.on_accent))
                .fill(palette.accent)
                .min_size(Vec2::new(button_width, height)),
        );

        if go.clicked() || (submitted && enabled) {
            app.enqueue_current_url();
        }
    });
}
