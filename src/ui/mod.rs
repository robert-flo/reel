//! La interfaz. Un modulo por franja de la ventana, en el orden en que se
//! dibujan, para que cada archivo quepa en una pantalla.

pub mod media_card;
pub mod queue;
pub mod settings;
pub mod status_bar;
pub mod top_bar;
pub mod url_bar;

use egui::{Color32, CornerRadius, Response, RichText, Sense, Stroke, Ui, Vec2};
use fastframe_fonts::Weight;

use crate::palette::Palette;

/// Texto con el peso correcto. fastframe-fonts trae Inter en 400, 500, 600 y
/// 700, asi que la jerarquia es de peso y no de tamanos arbitrarios.
pub fn text(content: impl Into<String>, size: f32, weight: Weight, color: Color32) -> RichText {
    RichText::new(content.into())
        .font(weight.font_id(size))
        .color(color)
}

pub fn caption(content: impl Into<String>, palette: &Palette) -> RichText {
    text(content, 11.0, Weight::Regular, palette.dim)
}

/// La pastilla de formato: Mejor, 4K, 1080p, mp3, opus.
pub fn chip(ui: &mut Ui, label: &str, selected: bool, palette: &Palette) -> Response {
    let padding = Vec2::new(14.0, 7.0);
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        Weight::Medium.font_id(11.0),
        if selected {
            palette.on_accent
        } else {
            palette.dim
        },
    );
    let size = galley.size() + padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let fill = match (selected, hovered) {
            (true, _) => palette.accent,
            (false, true) => palette.surface_hover,
            (false, false) => palette.surface,
        };
        let stroke = if selected {
            Stroke::NONE
        } else {
            Stroke::new(1.0, palette.outline)
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(13),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, palette.text);
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// La barra de progreso de la cola: fina, sin texto encima, con el color que
/// le corresponda al estado.
pub fn progress_bar(ui: &mut Ui, fraction: f32, color: Color32, palette: &Palette) {
    let height = 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let radius = CornerRadius::same(3);
    ui.painter()
        .rect_filled(rect, radius, palette.surface_hover);
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        let mut filled = rect;
        filled.set_width(rect.width() * fraction);
        ui.painter().rect_filled(filled, radius, color);
    }
}

/// Medidas compartidas, para no sembrar numeros magicos por toda la interfaz.
pub struct Metrics;

impl Metrics {
    pub const GUTTER: f32 = 20.0;
    pub const GAP: f32 = 10.0;
    pub const TOP_BAR: f32 = 44.0;
    pub const STATUS_BAR: f32 = 46.0;
    pub const URL_ROW: f32 = 48.0;
    pub const CARD: f32 = 140.0;
    pub const RADIUS: u8 = 10;
}

/// Un tiempo restante como lo diria una persona: "5:03" si falta menos de una
/// hora, "2h 05" si falta mas. Antes eran minutos sueltos, asi que una hora se
/// leia "60:00", que no le dice nada a nadie.
pub fn human_eta(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Bytes por segundo en unidades legibles.
pub fn human_speed(bytes_per_second: f64) -> String {
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut value = bytes_per_second;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Bytes en unidades legibles (B, KB, MB, GB).
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Una duracion de video como la escribe YouTube.
pub fn human_duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Memoria residente (RSS) del proceso actual en bytes.
///
/// En Linux lee `VmRSS` directamente desde `/proc/self/status`.
pub fn current_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let content = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb_str = rest.split_whitespace().next()?;
                let kb: u64 = kb_str.parse().ok()?;
                return Some(kb * 1024);
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocidades_legibles() {
        assert_eq!(human_speed(512.0), "512.0 B/s");
        assert_eq!(human_speed(1024.0 * 1024.0 * 14.2), "14.2 MB/s");
    }

    #[test]
    fn bytes_legibles() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024 * 15), "15.0 KB");
        assert_eq!(human_bytes(1024 * 1024 * 120), "120.0 MB");
        assert_eq!(human_bytes(1024 * 1024 * 1024 * 4), "4.0 GB");
    }

    #[test]
    fn duraciones_legibles() {
        assert_eq!(human_duration(213.0), "3:33");
        assert_eq!(human_duration(3725.0), "1:02:05");
    }

    #[test]
    fn eta_legible() {
        assert_eq!(human_eta(41), "0:41");
        assert_eq!(human_eta(82), "1:22");
        assert_eq!(human_eta(303), "5:03");
        // Lo que antes se leia "60:00".
        assert_eq!(human_eta(3600), "1h 00");
        assert_eq!(human_eta(7500), "2h 05");
    }

    #[test]
    fn memoria_en_linux() {
        if cfg!(target_os = "linux") {
            let mem = current_memory_bytes();
            assert!(mem.is_some());
            assert!(mem.unwrap() > 0);
        }
    }
}
