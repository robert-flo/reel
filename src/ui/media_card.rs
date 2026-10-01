//! La tarjeta de lo detectado: miniatura, titulo, autor y los formatos.
//!
//! Aparece en cuanto yt-dlp resuelve los metadatos del enlace pegado, antes
//! de que nadie decida descargar nada.

use egui::{Align, CornerRadius, Layout, Sense, Stroke, Vec2};
use fastframe_fonts::Weight;

use crate::app::App;
use crate::backend::{Kind, FORMATS};

use super::widgets::{caption, chip, text};
use super::{human_duration, Metrics};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    if app.probing && app.preview.is_none() {
        ui.label(caption("leyendo el enlace...", &palette));
        ui.add_space(Metrics::GAP);
        return;
    }

    if let Some(reason) = app.preview_error.clone() {
        egui::Frame::new()
            .fill(palette.panel)
            .stroke(Stroke::new(1.0, palette.outline))
            .corner_radius(CornerRadius::same(Metrics::RADIUS))
            .inner_margin(egui::Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.label(text(
                    "no pude leer ese enlace",
                    13.0,
                    Weight::Medium,
                    palette.danger,
                ));
                ui.add_space(4.0);
                ui.label(caption(reason, &palette));
            });
        ui.add_space(Metrics::GAP);
        return;
    }

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

                // El boton se reserva su ancho antes que el titulo, para que
                // un titulo largo lo recorte a el y no al reves.
                let button_width = 148.0;
                let text_width = (ui.available_width() - button_width - 16.0).max(180.0);

                ui.allocate_ui_with_layout(
                    Vec2::new(text_width, Metrics::CARD - 28.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(text(
                                &preview.title,
                                16.0,
                                Weight::SemiBold,
                                palette.text,
                            ))
                            .truncate(),
                        );
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

                        // `--no-playlist` no frena una url de playlist: se
                        // baja entera. Mejor decirlo antes de que alguien
                        // apriete el boton.
                        if let Some(cuantos) = preview.playlist_count {
                            ui.add_space(4.0);
                            ui.label(text(
                                format!(
                                    "es una lista: {cuantos} {}",
                                    if cuantos == 1 { "video" } else { "videos" }
                                ),
                                11.0,
                                Weight::Medium,
                                palette.warning,
                            ));
                        }

                        ui.add_space(12.0);
                        formats(app, ui);
                        ui.add_space(10.0);
                        extras(app, ui);
                    },
                );

                // "a la cola", pegado a la derecha y centrado en la tarjeta,
                // como en el boceto.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Una lista grande se confirma en dos toques: el primero
                    // pregunta y cambia el boton, el segundo encola. Un
                    // encolado de gigabytes no deberia salir de un clic que
                    // quiza se queria dar en otro lado.
                    let esperando = app.confirmar_lista == preview.playlist_count;
                    let etiqueta = match preview.playlist_count {
                        Some(cuantos) if App::pide_confirmacion(cuantos) && esperando => {
                            format!("confirmar {cuantos}")
                        }
                        Some(cuantos) => format!("encolar {cuantos}"),
                        None => "descargar".to_string(),
                    };
                    let go = ui.add_sized(
                        Vec2::new(140.0, 38.0),
                        egui::Button::new(text(
                            etiqueta,
                            13.0,
                            Weight::SemiBold,
                            palette.on_accent,
                        ))
                        .fill(palette.accent)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(8)),
                    );
                    if go.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        match preview.playlist_count {
                            // Primer toque en una lista grande: queda armado y
                            // no encola. El segundo confirma.
                            Some(cuantos) if App::pide_confirmacion(cuantos) && !esperando => {
                                app.confirmar_lista = Some(cuantos);
                            }
                            _ => {
                                app.confirmar_lista = None;
                                app.enqueue_preview();
                            }
                        }
                    }
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

    // Si la miniatura todavia no esta, o no se pudo traer, pintamos el play y
    // ya. egui dibujaria su propia marca de error y eso se ve roto.
    let painted = match url {
        Some(url) => {
            let image = egui::Image::new(url)
                .corner_radius(CornerRadius::same(8))
                .fit_to_exact_size(size);
            match image.load_for_size(ui.ctx(), size) {
                Ok(egui::load::TexturePoll::Ready { .. }) => {
                    image.paint_at(ui, rect);
                    true
                }
                _ => false,
            }
        }
        None => false,
    };

    if !painted {
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

fn formats(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for format in FORMATS {
            let selected = app.options.format_id == format.id;
            if chip(ui, format.label, selected, &palette).clicked() {
                app.select_format(format.id);
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
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut first = true;

        if format.kind == Kind::Video {
            let mut chapters = app.options.chapters;
            if toggle(ui, "capitulos", &mut chapters, &mut first, &palette) {
                app.options.chapters = chapters;
            }
        }

        let mut metadata = app.options.metadata;
        if toggle(
            ui,
            "metadatos + caratula",
            &mut metadata,
            &mut first,
            &palette,
        ) {
            app.options.metadata = metadata;
        }

        let mut subtitles = app.settings.subtitle_languages().is_some();
        if toggle(ui, "subtitulos", &mut subtitles, &mut first, &palette) {
            // Tocar aca es elegir un idioma, no apagarlos todos: los que haya
            // en el panel se quedan.
            app.toggle_subtitle("es");
        }

        let mut cookies = app.options.cookies_from_browser.is_some();
        if toggle(
            ui,
            "cookies del navegador",
            &mut cookies,
            &mut first,
            &palette,
        ) {
            app.options.cookies_from_browser = cookies.then(|| app.default_browser.clone());
        }
    });
}

/// Una opcion como texto: encendida se lee clara, apagada se apaga. Sin
/// casillas, que es lo que ensuciaba la fila.
fn toggle(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut bool,
    first: &mut bool,
    palette: &crate::palette::Palette,
) -> bool {
    if !*first {
        ui.label(text("  ·  ", 11.0, Weight::Regular, palette.outline));
    }
    *first = false;

    let color = if *value {
        palette.secondary
    } else {
        palette.dim
    };
    let response = ui
        .add(egui::Label::new(text(label, 11.0, Weight::Medium, color)).sense(Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    if response.clicked() {
        *value = !*value;
        return true;
    }
    false
}
