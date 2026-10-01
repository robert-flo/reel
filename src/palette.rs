//! La paleta de la app y su mapeo a `egui::Visuals`.
//!
//! fastframe-theme lee los archivos JSON, lista el catalogo y sigue el tema de
//! Omarchy. Lo que queda aqui es lo que fastframe deja deliberadamente a cada
//! app: que colores tiene, cuales son sus defaults claro y oscuro, y como se
//! traducen a los widgets de egui.

use std::collections::BTreeSet;

use egui::{Color32, Visuals};
use fastframe_theme::Base;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    // Los dieciseis nombres base que lee toda app de fastframe.
    pub window: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub surface_active: Color32,
    pub outline: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub overlay: Color32,
    pub shadow: Color32,
    // Propios de reel: los estados de la cola.
    pub progress: Color32,
    pub done: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            window: rgb(0x1a, 0x1b, 0x26),
            panel: rgb(0x1f, 0x20, 0x30),
            surface: rgb(0x24, 0x25, 0x3a),
            surface_hover: rgb(0x2b, 0x2d, 0x47),
            surface_active: rgb(0x32, 0x34, 0x52),
            outline: rgb(0x2f, 0x31, 0x50),
            text: rgb(0xc0, 0xca, 0xf5),
            secondary: rgb(0x9a, 0xa5, 0xce),
            dim: rgb(0x7f, 0x87, 0xb0),
            accent: rgb(0x7a, 0xa2, 0xf7),
            accent_hover: rgb(0x8f, 0xb3, 0xff),
            on_accent: rgb(0x16, 0x16, 0x1e),
            danger: rgb(0xf7, 0x76, 0x8e),
            warning: rgb(0xe0, 0xaf, 0x68),
            overlay: Color32::from_rgba_premultiplied(0, 0, 0, 160),
            shadow: Color32::from_rgba_premultiplied(0, 0, 0, 96),
            progress: rgb(0x7a, 0xa2, 0xf7),
            done: rgb(0x9e, 0xce, 0x6a),
        }
    }

    pub fn light() -> Self {
        Self {
            window: rgb(0xfa, 0xfa, 0xfc),
            panel: rgb(0xff, 0xff, 0xff),
            surface: rgb(0xf1, 0xf2, 0xf7),
            surface_hover: rgb(0xe7, 0xe9, 0xf2),
            surface_active: rgb(0xdd, 0xe0, 0xed),
            outline: rgb(0xd7, 0xda, 0xe6),
            text: rgb(0x1f, 0x22, 0x33),
            secondary: rgb(0x4b, 0x51, 0x6b),
            dim: rgb(0x7a, 0x80, 0x99),
            accent: rgb(0x34, 0x5c, 0xc8),
            accent_hover: rgb(0x29, 0x4c, 0xb0),
            on_accent: rgb(0xff, 0xff, 0xff),
            danger: rgb(0xc0, 0x3a, 0x52),
            warning: rgb(0xa5, 0x6c, 0x15),
            overlay: Color32::from_rgba_premultiplied(0, 0, 0, 64),
            shadow: Color32::from_rgba_premultiplied(0, 0, 0, 32),
            progress: rgb(0x34, 0x5c, 0xc8),
            done: rgb(0x3d, 0x7d, 0x2f),
        }
    }

    /// Visuals de egui a partir de la paleta. Todo lo visual de la app sale de
    /// aqui, para que cambiar de tema sea cambiar una sola estructura.
    pub fn visuals(&self) -> Visuals {
        let mut v = if self.is_dark() {
            Visuals::dark()
        } else {
            Visuals::light()
        };

        v.panel_fill = self.window;
        v.window_fill = self.panel;
        v.extreme_bg_color = self.surface;
        v.faint_bg_color = self.panel;
        v.override_text_color = Some(self.text);
        v.hyperlink_color = self.accent;
        v.selection.bg_fill = self.accent.linear_multiply(0.35);
        v.selection.stroke = egui::Stroke::new(1.0, self.accent);
        v.window_stroke = egui::Stroke::new(1.0, self.outline);

        let r = egui::CornerRadius::same(8);

        v.widgets.noninteractive.bg_fill = self.panel;
        v.widgets.noninteractive.weak_bg_fill = self.panel;
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, self.outline);
        v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, self.secondary);
        v.widgets.noninteractive.corner_radius = r;

        v.widgets.inactive.bg_fill = self.surface;
        v.widgets.inactive.weak_bg_fill = self.surface;
        v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, self.outline);
        v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, self.text);
        v.widgets.inactive.corner_radius = r;

        v.widgets.hovered.bg_fill = self.surface_hover;
        v.widgets.hovered.weak_bg_fill = self.surface_hover;
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, self.accent);
        v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, self.text);
        v.widgets.hovered.corner_radius = r;

        v.widgets.active.bg_fill = self.surface_active;
        v.widgets.active.weak_bg_fill = self.surface_active;
        v.widgets.active.bg_stroke = egui::Stroke::new(1.0, self.accent);
        v.widgets.active.fg_stroke = egui::Stroke::new(1.0, self.text);
        v.widgets.active.corner_radius = r;

        v.widgets.open = v.widgets.active;
        v
    }

    fn is_dark(&self) -> bool {
        let [r, g, b, _] = self.window.to_array();
        (r as u32 + g as u32 + b as u32) < 384
    }
}

impl fastframe_theme::Palette for Palette {
    fn base(base: Base) -> Self {
        match base {
            Base::Dark => Palette::dark(),
            Base::Light => Palette::light(),
        }
    }

    fn set(&mut self, name: &str, color: Color32) -> bool {
        match name {
            "window" => self.window = color,
            "panel" => self.panel = color,
            "surface" => self.surface = color,
            "surface_hover" => self.surface_hover = color,
            "surface_active" => self.surface_active = color,
            "outline" => self.outline = color,
            "text" => self.text = color,
            "secondary" => self.secondary = color,
            "dim" => self.dim = color,
            "accent" => self.accent = color,
            "accent_hover" => self.accent_hover = color,
            "on_accent" => self.on_accent = color,
            "danger" => self.danger = color,
            "warning" => self.warning = color,
            "overlay" => self.overlay = color,
            "shadow" => self.shadow = color,
            "progress" => self.progress = color,
            "done" => self.done = color,
            _ => return false,
        }
        true
    }

    /// Un archivo que solo define los dieciseis base todavia tiene que dar
    /// colores de cola decentes, asi que los derivamos.
    fn derive(&mut self, given: &BTreeSet<&str>) {
        if given.contains("accent") && !given.contains("progress") {
            self.progress = self.accent;
        }
        if given.contains("accent") && !given.contains("done") {
            self.done = self.accent;
        }
    }
}
