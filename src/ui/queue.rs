//! La cola. Es la pantalla principal, no un detalle: una fila por trabajo,
//! con su estado, su progreso y lo que esta haciendo yt-dlp ahora mismo.

use egui::{CornerRadius, Stroke};
use fastframe_fonts::Weight;

use crate::app::App;
use crate::backend::{Command, State};

use super::widgets::{caption, progress_bar, text};
use super::{human_eta, human_speed, Metrics};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;

    let (jobs, active, done) = {
        let queue = app.backend.queue.lock().unwrap_or_else(|e| e.into_inner());
        (queue.jobs.clone(), queue.active(), queue.done())
    };

    ui.horizontal(|ui| {
        ui.label(text("COLA", 11.0, Weight::SemiBold, palette.dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(caption(
                format!("{active} activas  ·  {done} completadas"),
                &palette,
            ));
        });
    });

    ui.add_space(8.0);

    if jobs.is_empty() {
        empty(ui, app);
        return;
    }

    let mut cancel: Option<u64> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for job in &jobs {
                row(ui, app, job, &mut cancel);
                ui.add_space(Metrics::GAP);
            }
        });

    if let Some(id) = cancel {
        app.backend.send(Command::Cancel { id });
    }
}

fn row(ui: &mut egui::Ui, app: &App, job: &crate::backend::Job, cancel: &mut Option<u64>) {
    let palette = app.palette;

    let (status, status_color, detail) = match &job.state {
        State::Probing => ("leyendo el enlace".into(), palette.dim, String::new()),
        State::Queued => ("en espera".into(), palette.dim, String::new()),
        State::Downloading => {
            let mut parts = Vec::new();
            if let Some(speed) = job.speed {
                parts.push(human_speed(speed));
            }
            if let Some(eta) = job.eta_secs {
                parts.push(format!("{} restante", human_eta(eta)));
            }
            ("descargando".to_string(), palette.accent, parts.join(" · "))
        }
        State::Postprocessing => (
            "esperando ffmpeg".to_string(),
            palette.warning,
            "fusionando pistas".to_string(),
        ),
        State::Done { path } => ("listo".to_string(), palette.done, path.clone()),
        State::Failed { reason } => ("fallo".to_string(), palette.danger, reason.clone()),
        State::Cancelled => ("cancelado".to_string(), palette.dim, String::new()),
    };

    let bar_color = match &job.state {
        State::Postprocessing => palette.warning,
        State::Done { .. } => palette.done,
        State::Failed { .. } => palette.danger,
        _ => palette.progress,
    };

    egui::Frame::new()
        .fill(palette.panel)
        .stroke(Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(Metrics::RADIUS))
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let title = if job.media.title.is_empty() {
                        job.url.clone()
                    } else {
                        job.media.title.clone()
                    };
                    ui.label(text(title, 13.0, Weight::SemiBold, palette.text));
                    ui.add_space(4.0);
                    ui.label(caption(
                        format!(
                            "{} · {}",
                            crate::backend::format_by_id(&job.options.format_id).label,
                            if job.media.host.is_empty() {
                                "—".to_string()
                            } else {
                                job.media.host.clone()
                            }
                        ),
                        &palette,
                    ));
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.vertical(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if job.is_active()
                                && ui
                                    .button(caption("cancelar", &palette))
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                            {
                                *cancel = Some(job.id);
                            }
                            ui.label(text(status, 11.0, Weight::Medium, status_color));
                        });
                        ui.add_space(4.0);
                        ui.label(caption(detail, &palette));
                    });
                });
            });

            ui.add_space(10.0);
            progress_bar(ui, job.progress, bar_color, &palette);
        });
}

fn empty(ui: &mut egui::Ui, app: &App) {
    let palette = app.palette;
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.label(text(
            "la cola esta vacia",
            14.0,
            Weight::Medium,
            palette.secondary,
        ));
        ui.add_space(6.0);
        ui.label(caption("pega un enlace arriba y aparece aqui", &palette));
    });
}
