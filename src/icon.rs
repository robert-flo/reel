//! Los iconos de la interfaz y el del tray.
//!
//! fastframe-icons incrusta los SVG y los sirve a egui con un cargador que no
//! olvida los bytes cuando egui recorta texturas.

fastframe_icons::icons! {
    /// Cada icono que dibuja la interfaz.
    pub enum Icon {
        prefix: "reel-icon-",
        directory: "../assets/icons/",
        Close => lucide "x",
        Check => lucide "check",
    }
}

pub fn install(ctx: &egui::Context) {
    fastframe_icons::install::<Icon>(ctx);
}

/// Icono de la ventana y del tray, en RGBA, al tamano que pida la plataforma.
/// Un carrete simple dibujado a mano: un circulo y cuatro perforaciones.
pub fn app_icon_rgba(size: usize) -> Vec<u8> {
    let mut pixels = vec![0u8; size * size * 4];
    let center = size as f32 / 2.0;
    let outer = center * 0.92;
    let inner = center * 0.22;
    let hole_orbit = center * 0.52;
    let hole_radius = center * 0.17;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let distance = (dx * dx + dy * dy).sqrt();

            let mut on = distance <= outer;
            if distance <= inner {
                on = false;
            }
            for quarter in 0..4 {
                let angle =
                    std::f32::consts::FRAC_PI_2 * quarter as f32 + std::f32::consts::FRAC_PI_4;
                let hx = angle.cos() * hole_orbit;
                let hy = angle.sin() * hole_orbit;
                let hd = ((dx - hx).powi(2) + (dy - hy).powi(2)).sqrt();
                if hd <= hole_radius {
                    on = false;
                }
            }

            let offset = (y * size + x) * 4;
            if on {
                pixels[offset] = 0x7a;
                pixels[offset + 1] = 0xa2;
                pixels[offset + 2] = 0xf7;
                pixels[offset + 3] = 0xff;
            }
        }
    }
    pixels
}

/// En la barra de menu de macOS el icono se tine solo, asi que va en blanco.
pub fn tray_template_rgba(size: usize) -> Vec<u8> {
    let mut pixels = app_icon_rgba(size);
    for chunk in pixels.as_chunks_mut::<4>().0 {
        if chunk[3] > 0 {
            chunk[0] = 0xff;
            chunk[1] = 0xff;
            chunk[2] = 0xff;
        }
    }
    pixels
}
