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
                    herramientas_corner(app, ui);
                    ui.add_space(16.0);
                    ui.label(caption(app.theme_label(), &palette));
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

    // Todavia no contestaron los hilos: mejor callarse que decir algo falso a
    // medias.
    if herramientas.ytdlp.is_none() && herramientas.ffmpeg.is_none() {
        return;
    }

    match resumen_de_herramientas(herramientas) {
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

fn resumen_de_herramientas(herramientas: &crate::app::Herramientas) -> Resumen {
    // yt-dlp primero: sin el no hay descargas, asi que su falla tapa a la otra.
    if let Some(Err(motivo)) = &herramientas.ytdlp {
        return Resumen::Falta {
            etiqueta: "sin yt-dlp no puedo bajar nada".into(),
            motivo: motivo.clone(),
        };
    }
    if let Some(Err(motivo)) = &herramientas.ffmpeg {
        return Resumen::Falta {
            etiqueta: "sin ffmpeg no puedo unir ni convertir".into(),
            motivo: format!(
                "{motivo}\n\nsin ffmpeg se baja el archivo tal como viene: no se unen pistas, no se extrae audio y no se incrustan metadatos"
            ),
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

    match &app.update {
        UpdateState::Idle | UpdateState::Checking => {}
        // Mientras no haya releases, callarse: no es una falla del usuario ni
        // algo que pueda arreglar.
        UpdateState::SinReleases => {}
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
        match resumen_de_herramientas(&herramientas) {
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
        match resumen_de_herramientas(&herramientas) {
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
    }

    /// Si faltan las dos, manda yt-dlp: sin el no hay descargas de nada.
    #[test]
    fn si_faltan_las_dos_manda_ytdlp() {
        let herramientas = Herramientas {
            ytdlp: falla("no encuentro yt-dlp"),
            ffmpeg: falla("no encuentro ffmpeg"),
        };
        match resumen_de_herramientas(&herramientas) {
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
            resumen_de_herramientas(&herramientas),
            Resumen::Esperando
        ));
    }
}
