//! Tipografia. Es lo primero que se configura, antes del primer frame:
//! Inter en cuatro pesos, fuentes instaladas para los alfabetos que Inter no
//! cubre, y el hinting y antialiasing que use el escritorio.

use fastframe_fonts::FontSetup;

pub fn setup(ctx: &egui::Context) {
    let mut fonts = FontSetup::default().definitions();

    let rendering = fastframe_text::detect();
    rendering.apply_to(&mut fonts);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| rendering.apply_to_visuals(&mut style.visuals));
}
