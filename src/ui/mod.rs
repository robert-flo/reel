//! La interfaz. Un modulo por franja de la ventana, en el orden en que se
//! dibujan, para que cada archivo quepa en una pantalla.

pub mod media_card;
pub mod queue;
pub mod status_bar;
pub mod top_bar;
pub mod url_bar;
pub mod widgets;

/// Medidas compartidas, para no sembrar numeros magicos por toda la interfaz.
pub struct Metrics;

impl Metrics {
    pub const GUTTER: f32 = 20.0;
    pub const GAP: f32 = 10.0;
    pub const TOP_BAR: f32 = 44.0;
    pub const STATUS_BAR: f32 = 46.0;
    pub const URL_ROW: f32 = 48.0;
    pub const CARD: f32 = 140.0;
    pub const QUEUE_ROW: f32 = 74.0;
    pub const RADIUS: u8 = 10;
}

/// Un tiempo en segundos como lo diria una persona.
pub fn human_eta(seconds: u64) -> String {
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    format!("{minutes:02}:{seconds:02}")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocidades_legibles() {
        assert_eq!(human_speed(512.0), "512.0 B/s");
        assert_eq!(human_speed(1024.0 * 1024.0 * 14.2), "14.2 MB/s");
    }

    #[test]
    fn duraciones_legibles() {
        assert_eq!(human_duration(213.0), "3:33");
        assert_eq!(human_duration(3725.0), "1:02:05");
    }

    #[test]
    fn eta_legible() {
        assert_eq!(human_eta(41), "00:41");
        assert_eq!(human_eta(82), "01:22");
    }
}
