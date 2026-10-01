//! Los pocos widgets propios que la interfaz repite. Todo lo demas es egui
//! de fabrica con los visuals que arma `Palette::visuals`.

use egui::{Color32, CornerRadius, Response, RichText, Sense, Stroke, Ui, Vec2};
use fastframe_fonts::Weight;

use crate::palette::Palette;

/// Texto con el peso correcto. fastframe-fonts trae Inter en 400, 500, 600 y
/// 700, asi que la jerarquia es de peso y no de tamanos arbitrarios.
pub fn text(content: impl Into<String>, size: f32, weight: Weight, color: Color32) -> RichText {
    RichText::new(content.into())
        .font(weight.font_id(size))
        .color(color)
}

pub fn caption(content: impl Into<String>, palette: &Palette) -> RichText {
    text(content, 11.0, Weight::Regular, palette.dim)
}

/// La pastilla de formato: Mejor, 4K, 1080p, mp3, opus.
pub fn chip(ui: &mut Ui, label: &str, selected: bool, palette: &Palette) -> Response {
    let padding = Vec2::new(14.0, 7.0);
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        Weight::Medium.font_id(11.0),
        if selected {
            palette.on_accent
        } else {
            palette.dim
        },
    );
    let size = galley.size() + padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let fill = match (selected, hovered) {
            (true, _) => palette.accent,
            (false, true) => palette.surface_hover,
            (false, false) => palette.surface,
        };
        let stroke = if selected {
            Stroke::NONE
        } else {
            Stroke::new(1.0, palette.outline)
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(13),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, palette.text);
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// La barra de progreso de la cola: fina, sin texto encima, con el color que
/// le corresponda al estado.
pub fn progress_bar(ui: &mut Ui, fraction: f32, color: Color32, palette: &Palette) {
    let height = 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let radius = CornerRadius::same(3);
    ui.painter()
        .rect_filled(rect, radius, palette.surface_hover);
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        let mut filled = rect;
        filled.set_width(rect.width() * fraction);
        ui.painter().rect_filled(filled, radius, color);
    }
}
