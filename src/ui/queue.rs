//! La cola. Es la pantalla principal, no un detalle: una fila por trabajo,
//! con su estado, su progreso y lo que esta haciendo yt-dlp ahora mismo.

use std::path::{Path, PathBuf};

use egui::{CornerRadius, Stroke};
use fastframe_fonts::Weight;

use crate::app::{App, QueueFilter};
use crate::backend::{Command, State};

use super::widgets::{caption, progress_bar, text};
use super::{human_eta, human_speed, Metrics};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let mut reintentar_todo: Option<()> = None;
    let mut limpiar_terminadas = false;

    let (jobs, active, done, failed) = {
        let queue = app.backend.queue.lock().unwrap_or_else(|e| e.into_inner());
        let failed = queue
            .jobs
            .iter()
            .filter(|job| matches!(job.state, State::Failed { .. }))
            .count();
        (queue.jobs.clone(), queue.active(), queue.done(), failed)
    };

    // Los que ya terminaron se pueden volver a pedir, y al reintentar se
    // reanuda lo que haya quedado a medias.
    let retryable = jobs.iter().filter(|job| !job.is_active()).count();

    ui.horizontal(|ui| {
        ui.label(text("COLA", 11.0, Weight::SemiBold, palette.dim));

        if jobs.len() > 1 {
            ui.add_space(8.0);
            let mut filter_chip =
                |ui: &mut egui::Ui, label: &str, count: usize, filter: QueueFilter| {
                    if count == 0 && app.queue_filter != filter {
                        return;
                    }
                    let is_selected = app.queue_filter == filter;
                    let text_label = format!("{label} ({count})");
                    let color = if is_selected {
                        palette.accent
                    } else if filter == QueueFilter::Failed && count > 0 {
                        palette.danger
                    } else {
                        palette.dim
                    };
                    let hit = ui
                        .add(
                            egui::Label::new(text(
                                text_label,
                                11.0,
                                if is_selected {
                                    Weight::SemiBold
                                } else {
                                    Weight::Regular
                                },
                                color,
                            ))
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if hit.clicked() {
                        app.queue_filter = if is_selected && filter != QueueFilter::All {
                            QueueFilter::All
                        } else {
                            filter
                        };
                    }
                    ui.add_space(6.0);
                };

            filter_chip(ui, "todas", jobs.len(), QueueFilter::All);
            filter_chip(ui, "activas", active, QueueFilter::Active);
            filter_chip(ui, "listas", done, QueueFilter::Done);
            if failed > 0 {
                filter_chip(ui, "con error", failed, QueueFilter::Failed);
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if retryable > 0 {
                let hit_reintentar = ui
                    .add(
                        egui::Label::new(text(
                            format!("reintentar todo ({retryable})"),
                            11.0,
                            Weight::Regular,
                            palette.dim,
                        ))
                        .sense(egui::Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if hit_reintentar.clicked() {
                    reintentar_todo = Some(());
                }

                ui.add_space(10.0);
                let hit_limpiar = ui
                    .add(
                        egui::Label::new(text(
                            "limpiar terminadas",
                            11.0,
                            Weight::Regular,
                            palette.dim,
                        ))
                        .sense(egui::Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if hit_limpiar.clicked() {
                    limpiar_terminadas = true;
                }
            }

            if jobs.len() <= 1 {
                let mut status_parts = Vec::new();
                status_parts.push(format!(
                    "{active} {}",
                    if active == 1 { "activa" } else { "activas" }
                ));
                status_parts.push(format!(
                    "{done} {}",
                    if done == 1 {
                        "completada"
                    } else {
                        "completadas"
                    }
                ));
                if failed > 0 {
                    status_parts.push(format!(
                        "{failed} {}",
                        if failed == 1 { "fallada" } else { "falladas" }
                    ));
                }
                ui.label(caption(status_parts.join("  ·  "), &palette));
            }
        });
    });

    ui.add_space(8.0);

    if jobs.is_empty() {
        empty(ui, app);
        return;
    }

    let mut actions = RowActions::default();

    let visible_jobs: Vec<&crate::backend::Job> = jobs
        .iter()
        .filter(|job| match app.queue_filter {
            QueueFilter::All => true,
            QueueFilter::Active => job.is_active(),
            QueueFilter::Done => matches!(job.state, State::Done { .. }),
            QueueFilter::Failed => matches!(job.state, State::Failed { .. }),
        })
        .collect();

    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if visible_jobs.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(28.0);
                    let msg = match app.queue_filter {
                        QueueFilter::Active => "no hay descargas activas",
                        QueueFilter::Done => "no hay descargas terminadas",
                        QueueFilter::Failed => "no hay descargas con error",
                        QueueFilter::All => "la cola esta vacia",
                    };
                    ui.label(text(msg, 12.0, Weight::Regular, palette.dim));
                });
            } else {
                for job in visible_jobs {
                    row(ui, app, job, &mut actions);
                    ui.add_space(Metrics::GAP);
                }
            }
        });

    if let Some(id) = actions.cancel {
        app.backend.send(Command::Cancel { id });
    }
    if let Some(()) = reintentar_todo {
        app.retry_failed();
    }
    if limpiar_terminadas {
        app.clear_finished_jobs();
    }
    if let Some(id) = actions.quitar {
        app.remove_job(id);
    }
    if let Some(id) = actions.retry {
        app.retry(id);
    }
    if let Some(id) = actions.retry_forzado {
        app.retry_forzado(id);
    }
    if let Some(path) = actions.abrir_archivo {
        abrir_archivo(&path);
    }
    if let Some((path, dir)) = actions.abrir_carpeta {
        abrir_carpeta(&path, dir.as_deref());
    }
}

/// Abre el archivo con el reproductor del sistema.
fn abrir_archivo(path: &str) {
    let p = Path::new(path);
    if p.exists() {
        if let Err(error) = std::process::Command::new("xdg-open").arg(p).spawn() {
            log::warn!("no pude abrir {}: {error}", p.display());
        }
    } else {
        log::warn!("el archivo no existe: {path}");
    }
}

/// Abre la carpeta que contiene el archivo, o la carpeta de salida si es un
/// resumen de lista.
fn abrir_carpeta(path: &str, output_dir: Option<&Path>) {
    let p = Path::new(path);
    let carpeta: Option<PathBuf> = if p.is_dir() {
        Some(p.to_path_buf())
    } else if let Some(padre) = p
        .parent()
        .filter(|padre| padre.is_dir() && !padre.as_os_str().is_empty())
    {
        Some(padre.to_path_buf())
    } else {
        output_dir
            .filter(|d| d.is_dir())
            .map(|dir| dir.to_path_buf())
    };

    if let Some(carpeta) = carpeta {
        if let Err(error) = std::process::Command::new("xdg-open").arg(&carpeta).spawn() {
            log::warn!("no pude abrir {}: {error}", carpeta.display());
        }
    } else {
        log::warn!("no encontre la carpeta para abrir: {path}");
    }
}

#[derive(Default)]
struct RowActions {
    cancel: Option<u64>,
    retry: Option<u64>,
    retry_forzado: Option<u64>,
    quitar: Option<u64>,
    abrir_archivo: Option<String>,
    abrir_carpeta: Option<(String, Option<PathBuf>)>,
}

fn row(ui: &mut egui::Ui, app: &App, job: &crate::backend::Job, actions: &mut RowActions) {
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
        State::Postprocessing { .. } => (
            "esperando ffmpeg".to_string(),
            palette.warning,
            // Lo que yt-dlp dijo que esta haciendo, si lo dijo.
            job.postprocessor
                .as_deref()
                .map(postprocessor_label)
                .unwrap_or("postprocesando")
                .to_string(),
        ),
        State::Done { path } => ("listo".to_string(), palette.done, path.clone()),
        State::Failed { reason } => (
            "fallo".to_string(),
            palette.danger,
            // El error de yt-dlp tal cual, y si lo reconocemos, que hacer.
            // El mensaje crudo no se esconde: es la unica forma de reportarlo.
            match crate::backend::ytdlp::consejo_para(reason) {
                Some(consejo) => format!("{reason}\n{consejo}"),
                None => reason.clone(),
            },
        ),
        State::Cancelled => ("cancelado".to_string(), palette.dim, String::new()),
    };

    let bar_color = match &job.state {
        State::Postprocessing { .. } => palette.warning,
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
                                    actions.cancel = Some(job.id);
                                }
                                ui.add_space(10.0);
                            }
                            // Un trabajo que ya no corre se puede volver a
                            // pedir; al reintentar, yt-dlp reanuda el `.part`.
                            if !job.is_active() {
                                let hit = ui
                                    .add(
                                        egui::Label::new(text(
                                            "reintentar",
                                            11.0,
                                            Weight::Regular,
                                            palette.dim,
                                        ))
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if hit.clicked() {
                                    actions.retry = Some(job.id);
                                }
                                ui.add_space(10.0);
                            }
                            // Un archivo que ya esta no se vuelve a bajar al
                            // reintentar; volver a bajar solo aplica a videos
                            // sueltos, no a la fila resumen de una lista.
                            if !job.options.playlist
                                && matches!(job.state, State::Done { .. })
                                && !job.options.force
                            {
                                let hit = ui
                                    .add(
                                        egui::Label::new(text(
                                            "volver a bajar",
                                            11.0,
                                            Weight::Regular,
                                            palette.dim,
                                        ))
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if hit.clicked() {
                                    actions.retry_forzado = Some(job.id);
                                }
                                ui.add_space(10.0);
                            }
                            // Un trabajo listo ofrece abrir el archivo o la carpeta.
                            if let State::Done { path } = &job.state {
                                let es_archivo_real = Path::new(path).is_file();
                                if es_archivo_real {
                                    let hit_carpeta = ui
                                        .add(
                                            egui::Label::new(text(
                                                "carpeta",
                                                11.0,
                                                Weight::Regular,
                                                palette.dim,
                                            ))
                                            .sense(egui::Sense::click()),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if hit_carpeta.clicked() {
                                        actions.abrir_carpeta =
                                            Some((path.clone(), job.options.output_dir.clone()));
                                    }
                                    ui.add_space(10.0);

                                    let hit_abrir = ui
                                        .add(
                                            egui::Label::new(text(
                                                "abrir",
                                                11.0,
                                                Weight::Regular,
                                                palette.dim,
                                            ))
                                            .sense(egui::Sense::click()),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if hit_abrir.clicked() {
                                        actions.abrir_archivo = Some(path.clone());
                                    }
                                    ui.add_space(10.0);
                                } else {
                                    let hit_carpeta = ui
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
                                    if hit_carpeta.clicked() {
                                        actions.abrir_carpeta =
                                            Some((path.clone(), job.options.output_dir.clone()));
                                    }
                                    ui.add_space(10.0);
                                }
                            }
                            // Quitar trabajo inactivo de la cola
                            if !job.is_active() {
                                let hit_quitar = ui
                                    .add(
                                        egui::Label::new(text(
                                            "quitar",
                                            11.0,
                                            Weight::Regular,
                                            palette.dim,
                                        ))
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if hit_quitar.clicked() {
                                    actions.quitar = Some(job.id);
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

/// El nombre del paso de yt-dlp como lo diria una persona.
fn postprocessor_label(postprocessor: &str) -> &'static str {
    match postprocessor {
        "Merger" => "fusionando pistas",
        "ExtractAudio" => "extrayendo el audio",
        "EmbedThumbnail" => "poniendo la caratula",
        _ => "postprocesando",
    }
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
