//! El pie: carpeta de salida, que tema se esta siguiendo, y la actualizacion
//! cuando hay una. Las tres cosas vienen de crates de fastframe.

use egui::{Align, Layout, Sense, Vec2};
use fastframe_fonts::Weight;

use crate::app::{App, UpdateState};

use super::{caption, text, Metrics};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let tr = app.tr();

    egui::Panel::bottom("status-bar")
        .exact_size(Metrics::STATUS_BAR)
        .frame(
            egui::Frame::new()
                .fill(palette.window)
                .inner_margin(egui::Margin::symmetric(Metrics::GUTTER as i8, 0)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let dir_hit = ui
                    .add(
                        egui::Label::new(caption(app.output_dir_label(), &palette))
                            .truncate()
                            .sense(Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tr.open_folder);
                if dir_hit.clicked() {
                    let dir = app.options.output_dir.clone().unwrap_or_else(|| {
                        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                        std::path::PathBuf::from(home).join("Videos")
                    });
                    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    update_corner(app, ui);
                    if ui.available_width() > 220.0 {
                        ui.add_space(16.0);
                        herramientas_corner(app, ui);
                    }
                    if ui.available_width() > 140.0 {
                        ui.add_space(16.0);
                        ui.label(caption(app.theme_label(), &palette));
                    }
                });
            });
        });
}

/// Las herramientas del sistema. Si andan, sus versiones quedan a la vista sin
/// gritar; la que falte se avisa en ambar, porque sin yt-dlp no se baja nada y
/// sin ffmpeg no se unen pistas ni se extrae audio.
fn herramientas_corner(app: &App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let herramientas = &app.herramientas;
    let tr = app.tr();

    // Todavia no contestaron los hilos: mejor callarse que decir algo falso a
    // medias.
    if herramientas.ytdlp.is_none() && herramientas.ffmpeg.is_none() {
        return;
    }

    match resumen_de_herramientas(herramientas, tr) {
        Resumen::Ok { ytdlp, ffmpeg } => {
            ui.label(caption(
                format!("yt-dlp {ytdlp} · ffmpeg {ffmpeg}"),
                &palette,
            ));
        }
        Resumen::Falta { etiqueta, motivo } => {
            ui.label(text(etiqueta, 11.0, Weight::Medium, palette.warning))
                .on_hover_text(motivo);
        }
        Resumen::Esperando => {}
    }
}

/// Que decir de las herramientas. Es una funcion aparte para poder probarla:
/// nombrar la herramienta equivocada es justo el error facil de cometer.
enum Resumen {
    Ok { ytdlp: String, ffmpeg: String },
    Falta { etiqueta: String, motivo: String },
    Esperando,
}

fn resumen_de_herramientas(
    herramientas: &crate::app::Herramientas,
    tr: &crate::i18n::Catalog,
) -> Resumen {
    // yt-dlp primero: sin el no hay descargas, asi que su falla tapa a la otra.
    if let Some(Err(motivo)) = &herramientas.ytdlp {
        return Resumen::Falta {
            etiqueta: tr.missing_ytdlp.into(),
            motivo: motivo.clone(),
        };
    }
    if let Some(Err(motivo)) = &herramientas.ffmpeg {
        return Resumen::Falta {
            etiqueta: tr.missing_ffmpeg.into(),
            motivo: format!("{motivo}\n\n{}", tr.missing_ffmpeg_extra),
        };
    }

    match (&herramientas.ytdlp, &herramientas.ffmpeg) {
        (Some(Ok(yt)), Some(Ok(ff))) => Resumen::Ok {
            ytdlp: yt.clone(),
            ffmpeg: ff.clone(),
        },
        // Una contesto y la otra no: se espera a tener las dos para hablar.
        _ => Resumen::Esperando,
    }
}

fn update_corner(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let tr = app.tr();

    match &app.update {
        UpdateState::Idle | UpdateState::Checking => {}
        // Mientras no haya releases, callarse: no es una falla del usuario ni
        // algo que pueda arreglar.
        UpdateState::SinReleases => {}
        UpdateState::Unsupported(reason) => {
            ui.label(caption(reason.clone(), &palette));
        }
        UpdateState::Available { version } => {
            let label = tr.update_available(version);
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
                tr.update_progress(percent),
                11.0,
                Weight::Medium,
                palette.accent,
            ));
        }
        UpdateState::Ready => {
            if ui
                .button(text(
                    tr.update_restart,
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
            ui.label(caption(tr.update_check_failed, &palette))
                .on_hover_text(reason.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Herramientas;

    fn ok(version: &str) -> Option<Result<String, String>> {
        Some(Ok(version.to_string()))
    }

    fn falla(motivo: &str) -> Option<Result<String, String>> {
        Some(Err(motivo.to_string()))
    }

    /// Las dos andando: se muestran las dos versiones juntas.
    #[test]
    fn con_las_dos_andando_muestra_las_dos_versiones() {
        let herramientas = Herramientas {
            ytdlp: ok("2026.08.19"),
            ffmpeg: ok("n9.0.2"),
        };
        match resumen_de_herramientas(&herramientas, crate::i18n::Language::En.catalog()) {
            Resumen::Ok { ytdlp, ffmpeg } => {
                assert_eq!(ytdlp, "2026.08.19");
                assert_eq!(ffmpeg, "n9.0.2");
            }
            _ => panic!("deberia estar todo bien"),
        }
    }

    /// Si falta ffmpeg, la etiqueta tiene que hablar de ffmpeg. Nombrar a
    /// yt-dlp aca fue un bug de verdad: el aviso salia en ambar pero decia
    /// "yt-dlp 2026.08.19", que no explica nada.
    #[test]
    fn si_falta_ffmpeg_lo_nombra_a_el() {
        let herramientas = Herramientas {
            ytdlp: ok("2026.08.19"),
            ffmpeg: falla("no encuentro ffmpeg"),
        };
        match resumen_de_herramientas(&herramientas, crate::i18n::Language::En.catalog()) {
            Resumen::Falta { etiqueta, motivo } => {
                assert!(
                    etiqueta.contains("ffmpeg"),
                    "etiqueta equivocada: {etiqueta}"
                );
                assert!(!etiqueta.contains("yt-dlp"), "no deberia culpar a yt-dlp");
                assert!(motivo.contains("no encuentro ffmpeg"));
            }
            _ => panic!("deberia avisar de ffmpeg"),
        }
        match resumen_de_herramientas(&herramientas, crate::i18n::Language::Es.catalog()) {
            Resumen::Falta { etiqueta, .. } => {
                assert!(
                    etiqueta.contains("ffmpeg"),
                    "etiqueta en espanol equivocada: {etiqueta}"
                );
                assert!(etiqueta.contains("unir") || etiqueta.contains("convertir"));
            }
            _ => panic!("deberia avisar de ffmpeg en espanol"),
        }
    }

    /// Si faltan las dos, manda yt-dlp: sin el no hay descargas de nada.
    #[test]
    fn si_faltan_las_dos_manda_ytdlp() {
        let herramientas = Herramientas {
            ytdlp: falla("no encuentro yt-dlp"),
            ffmpeg: falla("no encuentro ffmpeg"),
        };
        match resumen_de_herramientas(&herramientas, crate::i18n::Language::En.catalog()) {
            Resumen::Falta { etiqueta, .. } => {
                assert!(etiqueta.contains("yt-dlp"), "etiqueta: {etiqueta}");
            }
            _ => panic!("deberia avisar"),
        }
    }

    /// Con una sola respuesta se espera: decir media verdad es peor que
    /// callarse un instante.
    #[test]
    fn con_una_sola_respuesta_espera() {
        let herramientas = Herramientas {
            ytdlp: ok("2026.08.19"),
            ffmpeg: None,
        };
        assert!(matches!(
            resumen_de_herramientas(&herramientas, crate::i18n::Language::En.catalog()),
            Resumen::Esperando
        ));
    }
}
