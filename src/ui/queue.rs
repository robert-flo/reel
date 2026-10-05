//! La cola. Es la pantalla principal, no un detalle: una fila por trabajo,
//! con su estado, su progreso y lo que esta haciendo yt-dlp ahora mismo.

use std::path::{Path, PathBuf};

use egui::{CornerRadius, Stroke};
use fastframe_fonts::Weight;

use crate::app::{App, QueueFilter};
use crate::backend::{Command, State};

use super::{caption, human_eta, human_speed, progress_bar, text, Metrics};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let tr = app.tr();
    let mut reintentar_todo: Option<()> = None;
    let mut limpiar_terminadas = false;
    let mut cancelar_activas = false;
    let mut actions = RowActions::default();

    let (jobs, active, done, failed) = {
        let queue = app.backend.queue.lock().unwrap_or_else(|e| e.into_inner());
        let failed = queue
            .jobs
            .iter()
            .filter(|job| matches!(job.state, State::Failed { .. }))
            .count();
        (queue.jobs.clone(), queue.active(), queue.done(), failed)
    };

    // Trabajos inactivos y con error/cancelados para las acciones de cabecera.
    let inactive = jobs.iter().filter(|job| !job.is_active()).count();
    let failed_or_cancelled = jobs
        .iter()
        .filter(|job| matches!(job.state, State::Failed { .. } | State::Cancelled))
        .count();
    let total_speed: f64 = jobs
        .iter()
        .filter(|j| j.is_active())
        .filter_map(|j| j.speed)
        .sum();

    let is_compact_header = ui.available_width() < 520.0;
    ui.horizontal(|ui| {
        ui.label(text(tr.queue, 11.0, Weight::SemiBold, palette.dim));
        if total_speed > 0.0 {
            ui.add_space(4.0);
            ui.label(text(
                format!("· {}", crate::ui::human_speed(total_speed)),
                11.0,
                Weight::SemiBold,
                palette.accent,
            ));
        }

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

            filter_chip(ui, tr.filter_all, jobs.len(), QueueFilter::All);
            filter_chip(ui, tr.filter_active, active, QueueFilter::Active);
            filter_chip(ui, tr.filter_done, done, QueueFilter::Done);
            if failed > 0 {
                filter_chip(ui, tr.filter_failed, failed, QueueFilter::Failed);
            }
        }

        if jobs.len() > 3 {
            ui.add_space(6.0);
            let search_edit = egui::TextEdit::singleline(&mut app.queue_search)
                .id(egui::Id::new("queue_search_input"))
                .hint_text(text(tr.search_hint, 11.0, Weight::Regular, palette.dim))
                .font(Weight::Regular.font_id(11.0))
                .margin(egui::Margin::symmetric(6, 2))
                .desired_width(110.0);
            ui.add(search_edit);
            if !app.queue_search.is_empty() {
                let hit_x = ui
                    .add(
                        egui::Label::new(text("×", 12.0, Weight::SemiBold, palette.dim))
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if hit_x.clicked() {
                    app.queue_search.clear();
                }
            }
        }

        if !is_compact_header {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                render_header_actions(
                    ui,
                    app,
                    &palette,
                    tr,
                    active,
                    done,
                    failed,
                    failed_or_cancelled,
                    inactive,
                    &jobs,
                    &mut cancelar_activas,
                    &mut reintentar_todo,
                    &mut limpiar_terminadas,
                    &mut actions,
                );
            });
        }
    });

    if is_compact_header {
        ui.add_space(4.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            render_header_actions(
                ui,
                app,
                &palette,
                tr,
                active,
                done,
                failed,
                failed_or_cancelled,
                inactive,
                &jobs,
                &mut cancelar_activas,
                &mut reintentar_todo,
                &mut limpiar_terminadas,
                &mut actions,
            );
        });
    }

    ui.add_space(8.0);

    if jobs.is_empty() {
        empty(ui, app);
        return;
    }

    let query = app.queue_search.trim().to_lowercase();
    let visible_jobs: Vec<&crate::backend::Job> = jobs
        .iter()
        .filter(|job| {
            let matches_filter = match app.queue_filter {
                QueueFilter::All => true,
                QueueFilter::Active => job.is_active(),
                QueueFilter::Done => matches!(job.state, State::Done { .. }),
                QueueFilter::Failed => matches!(job.state, State::Failed { .. }),
            };
            let matches_query = query.is_empty()
                || job.media.title.to_lowercase().contains(&query)
                || job.url.to_lowercase().contains(&query);
            matches_filter && matches_query
        })
        .collect();

    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if visible_jobs.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(28.0);
                    let msg = if !query.is_empty() {
                        tr.no_match(&query)
                    } else {
                        match app.queue_filter {
                            QueueFilter::Active => tr.no_active.to_string(),
                            QueueFilter::Done => tr.no_done.to_string(),
                            QueueFilter::Failed => tr.no_failed.to_string(),
                            QueueFilter::All => tr.empty_title.to_string(),
                        }
                    };
                    ui.label(text(&msg, 12.0, Weight::Regular, palette.dim));
                    if !query.is_empty() {
                        ui.add_space(8.0);
                        let hit_clear = ui
                            .add(
                                egui::Label::new(text(
                                    tr.clear_search,
                                    11.0,
                                    Weight::Medium,
                                    palette.accent,
                                ))
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if hit_clear.clicked() {
                            app.queue_search.clear();
                        }
                    }
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
    if cancelar_activas {
        app.cancel_active_jobs();
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
    if let Some(texto) = actions.copiar_texto {
        app.copy_to_clipboard(ui.ctx(), &texto);
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
    copiar_texto: Option<String>,
}

fn row(ui: &mut egui::Ui, app: &App, job: &crate::backend::Job, actions: &mut RowActions) {
    let palette = app.palette;
    let tr = app.tr();

    let (status, status_color, detail) = match &job.state {
        State::Probing => (tr.status_probing.to_string(), palette.dim, String::new()),
        State::Queued => (tr.status_queued.to_string(), palette.dim, String::new()),
        State::Downloading => {
            let mut parts = Vec::new();
            if let Some(speed) = job.speed {
                parts.push(human_speed(speed));
            }
            if let Some(eta) = job.eta_secs {
                parts.push(tr.remaining_eta(&human_eta(eta)));
            }
            (
                tr.status_downloading.to_string(),
                palette.accent,
                parts.join(" · "),
            )
        }
        State::Postprocessing { .. } => (
            tr.status_waiting_ffmpeg.to_string(),
            palette.warning,
            job.postprocessor
                .as_deref()
                .map(|name| tr.postprocessor(name).to_string())
                .unwrap_or_else(|| tr.pp_generic.to_string()),
        ),
        State::Done { path } => {
            let detail = crate::i18n::parse_playlist_done(path)
                .map(|n| tr.playlist_done(n))
                .unwrap_or_else(|| path.clone());
            (tr.status_done.to_string(), palette.done, detail)
        }
        State::Retrying {
            reason, wait_ms, ..
        } => {
            let secs = wait_ms.div_ceil(1000).max(1);
            (
                tr.status_retrying.to_string(),
                palette.warning,
                format!("{}\n{}", tr.retrying_in(&human_eta(secs)), reason),
            )
        }
        State::Failed { reason } => (
            tr.status_failed.to_string(),
            palette.danger,
            match crate::backend::ytdlp::consejo_para(reason) {
                Some(consejo) => format!("{reason}\n{}", tr.consejo(consejo)),
                None => reason.clone(),
            },
        ),
        State::Cancelled => (tr.status_cancelled.to_string(), palette.dim, String::new()),
    };

    let bar_color = match &job.state {
        State::Postprocessing { .. } | State::Retrying { .. } => palette.warning,
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
            // Linea 1: Titulo a la izquierda (acotado al ancho disponible) y Estado a la derecha
            ui.horizontal(|ui| {
                let status_w = ui
                    .painter()
                    .layout_no_wrap(status.clone(), Weight::Medium.font_id(11.0), status_color)
                    .rect
                    .width();
                let status_text = text(status, 11.0, Weight::Medium, status_color);

                let title = if job.media.title.is_empty() {
                    tr.untitled.to_string()
                } else {
                    job.media.title.clone()
                };
                let es_listo = matches!(job.state, State::Done { .. });
                let avail_for_title = (ui.available_width() - status_w - 12.0).max(60.0);

                let mut title_label =
                    egui::Label::new(text(&title, 13.0, Weight::SemiBold, palette.text)).truncate();
                if es_listo {
                    title_label = title_label.sense(egui::Sense::click());
                }
                let resp = ui
                    .add_sized([avail_for_title, 18.0], title_label)
                    .on_hover_text(&title);
                if es_listo {
                    let resp = resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(tr.open);
                    if resp.double_clicked() {
                        if let State::Done { path } = &job.state {
                            if Path::new(path).is_file() {
                                actions.abrir_archivo = Some(path.clone());
                            } else {
                                actions.abrir_carpeta =
                                    Some((path.clone(), job.options.output_dir.clone()));
                            }
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(status_text);
                });
            });

            // Linea 2: Formato a la izquierda y Acciones a la derecha
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(caption(job_format_label(job, tr), &palette));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if job.is_active() {
                        let hit = ui
                            .add(
                                egui::Label::new(text(
                                    tr.cancel,
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
                                    tr.retry,
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
                                    tr.redownload,
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
                            let hit_copiar = ui
                                .add(
                                    egui::Label::new(text(
                                        tr.copy_path,
                                        11.0,
                                        Weight::Regular,
                                        palette.dim,
                                    ))
                                    .sense(egui::Sense::click()),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if hit_copiar.clicked() {
                                actions.copiar_texto = Some(path.clone());
                            }
                            ui.add_space(10.0);

                            let hit_carpeta = ui
                                .add(
                                    egui::Label::new(text(
                                        tr.folder,
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
                                        tr.open,
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
                                        tr.open_folder,
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
                    if let State::Failed { reason } = &job.state {
                        let hit_copiar = ui
                            .add(
                                egui::Label::new(text(
                                    tr.copy_error,
                                    11.0,
                                    Weight::Regular,
                                    palette.dim,
                                ))
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if hit_copiar.clicked() {
                            actions.copiar_texto = Some(reason.clone());
                        }
                        ui.add_space(10.0);
                    }
                    // Copiar enlace original del video/audio
                    if !job.media.url.is_empty() {
                        let hit_url = ui
                            .add(
                                egui::Label::new(text(
                                    tr.copy_url,
                                    11.0,
                                    Weight::Regular,
                                    palette.dim,
                                ))
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if hit_url.clicked() {
                            actions.copiar_texto = Some(job.media.url.clone());
                        }
                        ui.add_space(10.0);
                    }
                    // Quitar trabajo inactivo de la cola
                    if !job.is_active() {
                        let hit_quitar = ui
                            .add(
                                egui::Label::new(text(
                                    tr.remove,
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
                });
            });

            // Linea 3: Detalle / Mensaje de error / Progreso (a ancho completo)
            if !detail.is_empty() {
                ui.add_space(6.0);
                ui.label(caption(&detail, &palette));
            }

            ui.add_space(10.0);
            progress_bar(ui, job.progress, bar_color, &palette);
        });
}

/// "1080p · mp4" o "mp3 · 320k": la calidad y el contenedor que de verdad va a
/// quedar en el disco, que es lo unico que importa en una fila de la cola.
fn job_format_label(job: &crate::backend::Job, tr: &crate::i18n::Catalog) -> String {
    let format = crate::backend::format_by_id(&job.options.format_id);
    let label = tr.format_label(format.id, format.label);
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

    let base = match (container, quality) {
        (Some(container), Some(quality)) => format!("{container} · {quality}"),
        (Some(container), None) => format!("{} · {container}", label.to_lowercase()),
        (None, _) => label.to_lowercase(),
    };
    if let Some(sec) = &job.options.download_sections {
        let clean = sec.strip_prefix('*').unwrap_or(sec);
        format!("{base} · [{clean}]")
    } else {
        base
    }
}

fn empty(ui: &mut egui::Ui, app: &App) {
    let palette = app.palette;
    let tr = app.tr();
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.label(text(
            tr.empty_title,
            14.0,
            Weight::Medium,
            palette.secondary,
        ));
        ui.add_space(6.0);
        ui.label(caption(tr.empty_hint, &palette));
    });
}

#[allow(clippy::too_many_arguments)]
fn render_header_actions(
    ui: &mut egui::Ui,
    app: &App,
    palette: &crate::palette::Palette,
    tr: &crate::i18n::Catalog,
    active: usize,
    done: usize,
    failed: usize,
    failed_or_cancelled: usize,
    inactive: usize,
    jobs: &[crate::backend::Job],
    cancelar_activas: &mut bool,
    reintentar_todo: &mut Option<()>,
    limpiar_terminadas: &mut bool,
    actions: &mut RowActions,
) {
    if active > 1 {
        let hit_cancelar = ui
            .add(
                egui::Label::new(text(
                    format!("{} ({active})", tr.cancel_active),
                    11.0,
                    Weight::Regular,
                    palette.dim,
                ))
                .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if hit_cancelar.clicked() {
            *cancelar_activas = true;
        }
        ui.add_space(10.0);
    }

    if failed_or_cancelled > 0 {
        let hit_reintentar = ui
            .add(
                egui::Label::new(text(
                    format!("{} ({failed_or_cancelled})", tr.retry_failed),
                    11.0,
                    Weight::Regular,
                    palette.dim,
                ))
                .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if hit_reintentar.clicked() {
            *reintentar_todo = Some(());
        }
        ui.add_space(10.0);
    }

    if inactive > 0 {
        let hit_limpiar = ui
            .add(
                egui::Label::new(text(tr.clear_finished, 11.0, Weight::Regular, palette.dim))
                    .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if hit_limpiar.clicked() {
            *limpiar_terminadas = true;
        }
        ui.add_space(10.0);
    }

    let urls_con_contenido = jobs.iter().filter(|j| !j.url.is_empty()).count();
    if urls_con_contenido > 1 {
        let hit_copiar_todos = ui
            .add(
                egui::Label::new(text(tr.copy_all_links, 11.0, Weight::Regular, palette.dim))
                    .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tr.copy_all_links_tip);
        if hit_copiar_todos.clicked() {
            actions.copiar_texto = Some(app.queue_urls_text());
        }
        ui.add_space(10.0);
    }

    if jobs.len() <= 1 {
        let mut status_parts = Vec::new();
        status_parts.push(tr.counted(active, tr.active_one, tr.active_many));
        status_parts.push(tr.counted(done, tr.completed_one, tr.completed_many));
        if failed > 0 {
            status_parts.push(tr.counted(failed, tr.failed_one, tr.failed_many));
        }
        ui.label(caption(status_parts.join("  ·  "), palette));
    }
}
