//! El pie: carpeta de salida, que tema se esta siguiendo, y la actualizacion
//! cuando hay una. Las tres cosas vienen de crates de fastframe.

use egui::{Align, Layout, Sense, Vec2};
use fastframe_fonts::Weight;

use crate::app::{App, UpdateState};

use super::widgets::{caption, text};
use super::Metrics;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;

    egui::Panel::bottom("status-bar")
        .exact_size(Metrics::STATUS_BAR)
        .frame(
            egui::Frame::new()
                .fill(palette.window)
                .inner_margin(egui::Margin::symmetric(Metrics::GUTTER as i8, 0)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(caption(app.output_dir_label(), &palette));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    update_corner(app, ui);
                    ui.add_space(16.0);
                    ytdlp_corner(app, ui);
                    ui.add_space(16.0);
                    ui.label(caption(app.theme_label(), &palette));
                });
            });
        });
}

/// Solo dice algo cuando yt-dlp no se puede usar: si anda, su version queda en
/// el hover y el pie no se llena de ruido.
fn ytdlp_corner(app: &App, ui: &mut egui::Ui) {
    let palette = app.palette;
    match &app.ytdlp {
        Some(Ok(version)) => {
            ui.label(caption(format!("yt-dlp {version}"), &palette));
        }
        Some(Err(reason)) => {
            ui.label(text(
                "sin yt-dlp no puedo bajar nada",
                11.0,
                Weight::Medium,
                palette.warning,
            ))
            .on_hover_text(reason.clone());
        }
        // Todavia no contesto el hilo: mejor callarse que decir algo falso.
        None => {}
    }
}

fn update_corner(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;

    match &app.update {
        UpdateState::Idle | UpdateState::Checking => {}
        UpdateState::Unsupported(reason) => {
            ui.label(caption(reason.clone(), &palette));
        }
        UpdateState::Available { version } => {
            let label = format!("{version} disponible  ·  actualizar");
            let galley =
                ui.painter()
                    .layout_no_wrap(label, Weight::Medium.font_id(11.0), palette.done);
            let (rect, response) =
                ui.allocate_exact_size(galley.size() + Vec2::new(4.0, 0.0), Sense::click());
            ui.painter().galley(rect.left_top(), galley, palette.done);
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                app.start_update_download();
            }
        }
        UpdateState::Downloading { received, total } => {
            let percent = if *total > 0 {
                (*received as f32 / *total as f32 * 100.0).round() as u32
            } else {
                0
            };
            ui.label(text(
                format!("descargando actualizacion {percent}%"),
                11.0,
                Weight::Medium,
                palette.accent,
            ));
        }
        UpdateState::Ready => {
            if ui
                .button(text(
                    "reiniciar para actualizar",
                    11.0,
                    Weight::Medium,
                    palette.on_accent,
                ))
                .clicked()
            {
                app.apply_update();
            }
        }
        UpdateState::Failed(reason) => {
            // Mientras no haya releases publicados el servidor contesta 404, y
            // eso no es una falla que merezca ser lo unico rojo de la ventana:
            // se dice en gris y el detalle queda en el hover.
            ui.label(caption("no pude revisar actualizaciones", &palette))
                .on_hover_text(reason.clone());
        }
    }
}
