//! La cola. Es la pantalla principal, no un detalle: una fila por trabajo,
//! con su estado, su progreso y lo que esta haciendo yt-dlp ahora mismo.

use std::path::{Path, PathBuf};

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
                format!(
                    "{active} {}  ·  {done} {}",
                    if active == 1 { "activa" } else { "activas" },
                    if done == 1 {
                        "completada"
                    } else {
                        "completadas"
                    }
                ),
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
    let mut abrir: Option<String> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for job in &jobs {
                row(ui, app, job, &mut cancel, &mut abrir);
                ui.add_space(Metrics::GAP);
            }
        });

    if let Some(id) = cancel {
        app.backend.send(Command::Cancel { id });
    }
    if let Some(path) = abrir {
        revelar(&path);
    }
}

/// Abre la carpeta del archivo que se bajo, que es lo que uno quiere hacer
/// despues. El trabajo pesado lo hace el escritorio con `xdg-open`; aca solo
/// se elige que abrir: la carpeta si se puede, el archivo si no.
fn revelar(path: &str) {
    let carpeta = Path::new(path)
        .parent()
        .filter(|padre| padre.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(path));

    if let Err(error) = std::process::Command::new("xdg-open").arg(&carpeta).spawn() {
        log::warn!("no pude abrir {}: {error}", carpeta.display());
    }
}

fn row(
    ui: &mut egui::Ui,
    app: &App,
    job: &crate::backend::Job,
    cancel: &mut Option<u64>,
    abrir: &mut Option<String>,
) {
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
                    ui.add(
                        egui::Label::new(text(title, 13.0, Weight::SemiBold, palette.text))
                            .truncate(),
                    );
                    ui.add_space(4.0);
                    ui.label(caption(job_format_label(job), &palette));
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.vertical(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if job.is_active() {
                                let hit = ui
                                    .add(
                                        egui::Label::new(text(
                                            "cancelar",
                                            11.0,
                                            Weight::Regular,
                                            palette.dim,
                                        ))
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if hit.clicked() {
                                    *cancel = Some(job.id);
                                }
                                ui.add_space(10.0);
                            }
                            // Un trabajo listo ofrece abrir donde quedo.
                            if let State::Done { path } = &job.state {
                                let hit = ui
                                    .add(
                                        egui::Label::new(text(
                                            "abrir carpeta",
                                            11.0,
                                            Weight::Regular,
                                            palette.dim,
                                        ))
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if hit.clicked() {
                                    *abrir = Some(path.clone());
                                }
                                ui.add_space(10.0);
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

/// "1080p · mp4" o "mp3 · 320k": la calidad y el contenedor que de verdad va a
/// quedar en el disco, que es lo unico que importa en una fila de la cola.
fn job_format_label(job: &crate::backend::Job) -> String {
    let format = crate::backend::format_by_id(&job.options.format_id);
    let container = format
        .args
        .windows(2)
        .find(|pair| pair[0] == "--merge-output-format" || pair[0] == "--audio-format")
        .map(|pair| pair[1].to_string());
    let quality = format
        .args
        .windows(2)
        .find(|pair| pair[0] == "--audio-quality")
        .map(|pair| pair[1].to_string());

    match (container, quality) {
        (Some(container), Some(quality)) => format!("{container} · {quality}"),
        (Some(container), None) => format!("{} · {container}", format.label.to_lowercase()),
        (None, _) => format.label.to_lowercase(),
    }
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
