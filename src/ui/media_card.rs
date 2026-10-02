//! La tarjeta de lo detectado: miniatura, titulo, autor y los formatos.
//!
//! Aparece en cuanto yt-dlp resuelve los metadatos del enlace pegado, antes
//! de que nadie decida descargar nada.

use egui::{Align, CornerRadius, Layout, Sense, Stroke, Vec2};
use fastframe_fonts::Weight;

use crate::app::App;
use crate::backend::{Kind, FORMATS};

use super::{caption, chip, human_duration, text, Metrics};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let tr = app.tr();
    if app.probing && app.preview.is_none() {
        egui::Frame::new()
            .fill(palette.panel)
            .stroke(Stroke::new(1.0, palette.outline))
            .corner_radius(CornerRadius::same(Metrics::RADIUS))
            .inner_margin(egui::Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.add_space(8.0);
                    ui.label(text(tr.reading_link, 13.0, Weight::Regular, palette.dim));
                });
            });
        ui.add_space(Metrics::GAP);
        return;
    }

    if let Some(reason) = app.preview_error.clone() {
        let consejo = crate::backend::ytdlp::consejo_para(&reason);
        let is_no_media = consejo == Some(crate::backend::ytdlp::Consejo::NoMedia);
        let headline = if is_no_media {
            tr.no_media_found
        } else {
            tr.could_not_read
        };
        let tip = match consejo {
            Some(crate::backend::ytdlp::Consejo::NoMedia) => None,
            Some(c) => Some(tr.consejo(c)),
            None => None,
        };

        egui::Frame::new()
            .fill(palette.panel)
            .stroke(Stroke::new(1.0, palette.outline))
            .corner_radius(CornerRadius::same(Metrics::RADIUS))
            .inner_margin(egui::Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(text(headline, 13.0, Weight::Medium, palette.danger));
                        if let Some(tip_text) = tip {
                            ui.add_space(4.0);
                            ui.label(caption(tip_text, &palette));
                        }
                        if reason != headline {
                            ui.add_space(4.0);
                            ui.label(caption(&reason, &palette));
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let hit_dismiss = ui
                            .add(
                                egui::Label::new(text(
                                    tr.dismiss,
                                    11.0,
                                    Weight::Regular,
                                    palette.dim,
                                ))
                                .sense(Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if hit_dismiss.clicked() {
                            app.preview_error = None;
                        }

                        ui.add_space(10.0);

                        let hit_copy = ui
                            .add(
                                egui::Label::new(text(
                                    tr.copy_error,
                                    11.0,
                                    Weight::Regular,
                                    palette.dim,
                                ))
                                .sense(Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if hit_copy.clicked() {
                            app.copy_to_clipboard(ui.ctx(), &reason);
                        }
                    });
                });
            });
        ui.add_space(Metrics::GAP);
        return;
    }

    let Some(preview) = app.preview.clone() else {
        return;
    };

    let available_w = ui.available_width();
    let is_wide = available_w >= 620.0;
    let thumb_size = if available_w < 480.0 {
        let w = (available_w * 0.4).clamp(140.0, 220.0);
        let h = (w * 9.0 / 16.0).round();
        Vec2::new(w, h)
    } else {
        Vec2::new(220.0, Metrics::CARD - 28.0)
    };

    egui::Frame::new()
        .fill(palette.panel)
        .stroke(Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(Metrics::RADIUS))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            if is_wide {
                ui.horizontal(|ui| {
                    thumbnail(ui, app, preview.thumbnail_url.as_deref(), thumb_size);
                    ui.add_space(14.0);

                    let button_width = 140.0;
                    let text_width = ui.available_width() - button_width - 14.0;

                    ui.allocate_ui_with_layout(
                        Vec2::new(text_width, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            render_card_info(app, ui, &preview);
                        },
                    );

                    ui.add_space(14.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        render_download_button(app, ui, &preview, Vec2::new(button_width, 38.0));
                    });
                });
            } else {
                ui.horizontal(|ui| {
                    thumbnail(ui, app, preview.thumbnail_url.as_deref(), thumb_size);
                    ui.add_space(14.0);

                    ui.vertical(|ui| {
                        render_card_info(app, ui, &preview);
                        ui.add_space(14.0);
                        let btn_width = 140.0f32.min(ui.available_width());
                        render_download_button(app, ui, &preview, Vec2::new(btn_width, 38.0));
                    });
                });
            }
        });
}

fn render_card_info(app: &mut App, ui: &mut egui::Ui, preview: &crate::backend::Media) {
    let palette = app.palette;
    let tr = app.tr();

    ui.add_space(4.0);
    let title = if preview.title.is_empty() {
        tr.untitled.to_string()
    } else {
        preview.title.clone()
    };
    ui.add(egui::Label::new(text(&title, 16.0, Weight::SemiBold, palette.text)).truncate())
        .on_hover_text(&title);
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
    if preview.playlist_count.is_none() {
        if let Some(filesize) = preview.filesize {
            meta.push(super::human_bytes(filesize));
        }
    }
    ui.label(caption(meta.join("  ·  "), &palette));

    if let Some(cuantos) = preview.playlist_count {
        ui.add_space(4.0);
        let mut info_lista = Vec::new();
        info_lista.push(tr.n_videos(cuantos));
        if let Some(duration) = preview.duration {
            info_lista.push(human_duration(duration));
        }
        if let Some(filesize) = preview.filesize {
            info_lista.push(format!("~{}", super::human_bytes(filesize)));
        }
        ui.label(text(
            format!("{} {}", tr.playlist_prefix, info_lista.join("  ·  ")),
            11.0,
            Weight::Medium,
            palette.warning,
        ));
    }

    ui.add_space(12.0);
    formats(app, ui);
    ui.add_space(10.0);
    extras(app, ui);
}

fn render_download_button(
    app: &mut App,
    ui: &mut egui::Ui,
    preview: &crate::backend::Media,
    size: Vec2,
) {
    let palette = app.palette;
    let tr = app.tr();

    let pide_confirmar = preview
        .playlist_count
        .is_some_and(|cuantos| App::pide_confirmacion(cuantos, preview.filesize));
    let esperando = app.confirmar_lista == preview.playlist_count;
    let etiqueta = match preview.playlist_count {
        Some(cuantos) if pide_confirmar && esperando => tr.confirm_n(cuantos),
        Some(cuantos) => tr.enqueue_n(cuantos),
        None => tr.download.to_string(),
    };
    let go = ui.add_sized(
        size,
        egui::Button::new(text(etiqueta, 13.0, Weight::SemiBold, palette.on_accent))
            .fill(palette.accent)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8)),
    );
    if go.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        match preview.playlist_count {
            Some(cuantos) if pide_confirmar && !esperando => {
                app.confirmar_lista = Some(cuantos);
            }
            _ => {
                app.confirmar_lista = None;
                app.enqueue_preview();
            }
        }
    }
}

fn thumbnail(ui: &mut egui::Ui, app: &App, url: Option<&str>, size: Vec2) {
    let palette = app.palette;
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
    let tr = app.tr();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for format in FORMATS {
            let selected = app.options.format_id == format.id;
            if chip(
                ui,
                tr.format_label(format.id, format.label),
                selected,
                &palette,
            )
            .clicked()
            {
                app.select_format(format.id);
            }
        }
    });
}

/// Las opciones que yoinks y el plugin de la barra tienen fijas: capitulos,
/// metadatos, subtitulos y cookies del navegador.
fn extras(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let tr = app.tr();
    let format = crate::backend::format_by_id(&app.options.format_id);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut first = true;

        if format.kind == Kind::Video {
            let mut chapters = app.options.chapters;
            if toggle(ui, tr.chapters, &mut chapters, &mut first, &palette, None) {
                app.options.chapters = chapters;
            }

            let mut sponsorblock = app.options.sponsorblock;
            if toggle(
                ui,
                tr.sponsorblock,
                &mut sponsorblock,
                &mut first,
                &palette,
                Some(tr.sponsorblock_tip),
            ) {
                app.options.sponsorblock = sponsorblock;
            }
        }

        let mut metadata = app.options.metadata;
        if toggle(
            ui,
            tr.metadata_artwork,
            &mut metadata,
            &mut first,
            &palette,
            None,
        ) {
            app.options.metadata = metadata;
        }

        let mut subtitles = app.settings.subtitle_languages().is_some();
        if toggle(ui, tr.subtitles, &mut subtitles, &mut first, &palette, None) {
            // Tocar aca es elegir un idioma, no apagarlos todos: los que haya
            // en el panel se quedan.
            app.toggle_subtitle("es");
        }

        let mut cookies = app.options.cookies_from_browser.is_some();
        if toggle(
            ui,
            tr.browser_cookies,
            &mut cookies,
            &mut first,
            &palette,
            None,
        ) {
            app.options.cookies_from_browser = cookies.then(|| app.default_browser.clone());
        }

        let es_video_suelto = app
            .preview
            .as_ref()
            .is_none_or(|m| m.playlist_count.is_none());
        if es_video_suelto {
            let mut clip = app.clip_enabled;
            if toggle(
                ui,
                tr.clip,
                &mut clip,
                &mut first,
                &palette,
                Some(tr.clip_tip),
            ) {
                app.clip_enabled = clip;
            }
        }
    });

    let es_video_suelto = app
        .preview
        .as_ref()
        .is_none_or(|m| m.playlist_count.is_none());
    if es_video_suelto && app.clip_enabled {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(text(tr.clip_from, 11.0, Weight::Regular, palette.dim));
            ui.add(
                egui::TextEdit::singleline(&mut app.clip_start)
                    .hint_text("00:00")
                    .desired_width(50.0)
                    .font(Weight::Regular.font_id(11.0))
                    .margin(egui::Margin::symmetric(6, 2)),
            );
            ui.add_space(8.0);
            ui.label(text(tr.clip_to, 11.0, Weight::Regular, palette.dim));
            ui.add(
                egui::TextEdit::singleline(&mut app.clip_end)
                    .hint_text("05:00")
                    .desired_width(50.0)
                    .font(Weight::Regular.font_id(11.0))
                    .margin(egui::Margin::symmetric(6, 2)),
            );
        });
    }
}

/// Una opcion como texto: encendida se lee clara, apagada se apaga. Sin
/// casillas, que es lo que ensuciaba la fila.
fn toggle(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut bool,
    first: &mut bool,
    palette: &crate::palette::Palette,
    tooltip: Option<&str>,
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
    let mut response = ui
        .add(egui::Label::new(text(label, 11.0, Weight::Medium, color)).sense(Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if let Some(tip) = tooltip {
        response = response.on_hover_text(tip);
    }

    if response.clicked() {
        *value = !*value;
        return true;
    }
    false
}
