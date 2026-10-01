//! La tarjeta de lo detectado: miniatura, titulo, autor y los formatos.
//!
//! Aparece en cuanto yt-dlp resuelve los metadatos del enlace pegado, antes
//! de que nadie decida descargar nada.

use egui::{CornerRadius, Sense, Stroke, Vec2};
use fastframe_fonts::Weight;

use crate::app::App;
use crate::backend::{Kind, FORMATS};

use super::widgets::{caption, chip, text};
use super::{human_duration, Metrics};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let Some(preview) = app.preview.clone() else {
        return;
    };

    egui::Frame::new()
        .fill(palette.panel)
        .stroke(Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(Metrics::RADIUS))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                thumbnail(ui, app, preview.thumbnail_url.as_deref());
                ui.add_space(14.0);

                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.label(text(&preview.title, 16.0, Weight::SemiBold, palette.text));
                    ui.add_space(6.0);

                    let mut meta = Vec::new();
                    if !preview.uploader.is_empty() {
                        meta.push(preview.uploader.clone());
                    }
                    if let Some(duration) = preview.duration {
                        meta.push(human_duration(duration));
                    }
                    if !preview.host.is_empty() {
                        meta.push(preview.host.clone());
                    }
                    ui.label(caption(meta.join("  ·  "), &palette));

                    ui.add_space(12.0);
                    formats(app, ui);
                    ui.add_space(10.0);
                    extras(app, ui);
                });
            });
        });
}

fn thumbnail(ui: &mut egui::Ui, app: &App, url: Option<&str>) {
    let palette = app.palette;
    let size = Vec2::new(220.0, Metrics::CARD - 28.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }

    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), palette.surface_hover);

    match url {
        Some(url) => {
            egui::Image::new(url)
                .corner_radius(CornerRadius::same(8))
                .fit_to_exact_size(size)
                .paint_at(ui, rect);
        }
        None => {
            // Un triangulo de play mientras no hay miniatura.
            let center = rect.center();
            let half = 16.0;
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    center + Vec2::new(-half * 0.6, -half),
                    center + Vec2::new(-half * 0.6, half),
                    center + Vec2::new(half, 0.0),
                ],
                palette.dim,
                Stroke::NONE,
            ));
        }
    }
}

fn formats(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for format in FORMATS {
            let selected = app.options.format_id == format.id;
            if chip(ui, format.label, selected, &palette).clicked() {
                app.options.format_id = format.id.to_string();
            }
        }
    });
}

/// Las opciones que yoinks y el plugin de la barra tienen fijas: capitulos,
/// metadatos, subtitulos y cookies del navegador.
fn extras(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let format = crate::backend::format_by_id(&app.options.format_id);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        if format.kind == Kind::Video {
            ui.checkbox(&mut app.options.chapters, caption("capitulos", &palette));
            ui.label(caption("·", &palette));
        }

        let mut subtitles = app.options.subtitles.is_some();
        if ui
            .checkbox(&mut subtitles, caption("subtitulos es", &palette))
            .changed()
        {
            app.options.subtitles = subtitles.then(|| "es".to_string());
        }

        ui.label(caption("·", &palette));

        let mut cookies = app.options.cookies_from_browser.is_some();
        if ui
            .checkbox(&mut cookies, caption("cookies del navegador", &palette))
            .changed()
        {
            app.options.cookies_from_browser = cookies.then(|| app.default_browser.clone());
        }
    });
}
