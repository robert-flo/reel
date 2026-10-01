//! El panel de ajustes: lo que el README promete y hasta ahora no tenia
//! donde vivir.
//!
//! Es un modal de egui, asi que el velo de atras, el Escape y los clics de
//! afuera vienen puestos. Dentro estan las tres cosas que yoinks tiene fijas
//! (carpeta de salida, nombre del archivo y cookies del navegador) mas el
//! tema, que es la otra mitad del selector que faltaba.
//!
//! Los campos de texto se editan en un borrador y se aplican al confirmar o
//! al salir del campo: validar la carpeta en cada tecla daria un "no existe"
//! rojo mientras se escribe a medias.

use egui::{Align, Color32, CornerRadius, Layout, Sense, Stroke, Vec2};
use fastframe_fonts::Weight;

use crate::app::App;
use crate::settings::{self, DirProblem};

use super::widgets::{chip, text};
use super::Metrics;

/// El ancho del panel: es una ventana de ajustes, no una fila.
const PANEL_WIDTH: f32 = 640.0;
/// El ancho de los campos de texto, que van de lado a lado.
const FIELD_WIDTH: f32 = 600.0;
/// Cuantas muestras de tema entran por fila.
const THEMES_PER_ROW: usize = 4;

/// Dibuja el panel. Devuelve `true` cuando hay que cerrarlo: Escape, un clic
/// en el velo, o la cruz.
pub fn show(app: &mut App, ctx: &egui::Context) -> bool {
    let palette = app.palette;
    let response = egui::Modal::new(egui::Id::new("reel-settings"))
        .backdrop_color(palette.overlay)
        .frame(
            egui::Frame::new()
                .fill(palette.panel)
                .stroke(Stroke::new(1.0, palette.outline))
                .corner_radius(CornerRadius::same(Metrics::RADIUS))
                .inner_margin(egui::Margin::same(20)),
        )
        .show(ctx, |ui| {
            ui.set_min_width(PANEL_WIDTH);
            let close = header(ui, app, &palette);
            separator(ui, &palette);
            language(ui, app, &palette);
            ui.add_space(16.0);
            output_dir(ui, app, &palette);
            ui.add_space(16.0);
            format(ui, app, &palette);
            ui.add_space(16.0);
            filename(ui, app, &palette);
            ui.add_space(16.0);
            rate_limit(ui, app, &palette);
            ui.add_space(16.0);
            subtitles(ui, app, &palette);
            ui.add_space(16.0);
            cookies(ui, app, &palette);
            ui.add_space(16.0);
            extras_setting(ui, app, &palette);
            ui.add_space(16.0);
            advanced_setting(ui, app, &palette);
            ui.add_space(16.0);
            themes(ui, app, &palette);
            close
        });

    if response.inner {
        return true;
    }
    if response.should_close() {
        // Cerrar confirma lo que este escrito en los campos.
        commit(app);
        return true;
    }
    false
}

/// Aplica los campos de texto del panel a los ajustes. Se llama al cerrar y
/// cuando el foco sale de un campo.
fn commit(app: &mut App) {
    let changed = app.settings.output_dir != app.draft.output_dir
        || app.settings.filename_template != app.draft.filename_template
        || app.settings.rate_limit != app.draft.rate_limit
        || app.settings.sponsorblock != app.draft.sponsorblock
        || app.settings.chapters != app.draft.chapters
        || app.settings.metadata != app.draft.metadata
        || app.settings.extra_args != app.draft.extra_args
        || app.settings.subtitle_list() != app.draft_subtitles;
    if changed {
        app.settings.output_dir = app.draft.output_dir.clone();
        app.settings.filename_template = app.draft.filename_template.clone();
        app.settings.rate_limit = app.draft.rate_limit.clone();
        app.settings.sponsorblock = app.draft.sponsorblock;
        app.settings.chapters = app.draft.chapters;
        app.settings.metadata = app.draft.metadata;
        app.settings.extra_args = app.draft.extra_args.clone();
        app.settings.set_subtitles(&app.draft_subtitles.clone());
        app.settings_changed();
    }
}

fn header(ui: &mut egui::Ui, app: &App, palette: &crate::palette::Palette) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(text(
            app.tr().settings,
            15.0,
            Weight::SemiBold,
            palette.text,
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let cross = ui
                .add(
                    egui::Image::new(crate::icon::Icon::Close.uri())
                        .fit_to_exact_size(Vec2::splat(14.0))
                        .tint(palette.dim),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if cross.clicked() {
                close = true;
            }
        });
    });
    close
}

fn separator(ui: &mut egui::Ui, palette: &crate::palette::Palette) {
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, palette.outline);
    ui.add_space(12.0);
}

fn section(ui: &mut egui::Ui, title: &str, palette: &crate::palette::Palette) {
    ui.label(text(title, 11.0, Weight::SemiBold, palette.dim));
    ui.add_space(6.0);
}

/// Una ayuda corta debajo de un campo.
fn hint(ui: &mut egui::Ui, message: &str, color: Color32) {
    ui.add_space(4.0);
    ui.label(text(message, 11.0, Weight::Regular, color));
}

/// El idioma de la interfaz. Se aplica al instante y se guarda con el resto.
fn language(ui: &mut egui::Ui, app: &mut App, palette: &crate::palette::Palette) {
    let tr = app.tr();
    section(ui, tr.section_language, palette);

    let actual = app.settings.language;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for idioma in crate::i18n::Language::ALL {
            if chip(ui, idioma.native_name(), actual == *idioma, palette).clicked() {
                app.select_language(*idioma);
            }
        }
    });
}

/// La carpeta donde yt-dlp deja los archivos. Vacia es la del sistema.
fn output_dir(ui: &mut egui::Ui, app: &mut App, palette: &crate::palette::Palette) {
    let tr = app.tr();
    section(ui, tr.section_output, palette);

    let field = ui.add(
        egui::TextEdit::singleline(&mut app.draft.output_dir)
            .hint_text(text("~/Videos", 12.0, Weight::Regular, palette.dim))
            .font(Weight::Regular.font_id(12.0))
            .margin(egui::Margin::symmetric(12, 8))
            .desired_width(FIELD_WIDTH),
    );
    if field.lost_focus() {
        commit(app);
    }

    match app
        .draft
        .output_path()
        .and_then(|path| settings::check_dir(&path))
    {
        None => match app.draft.output_path() {
            Some(path) => hint(ui, &tr.output_saved(path.display()), palette.dim),
            None => hint(ui, tr.output_empty, palette.dim),
        },
        Some(DirProblem::Missing) => hint(ui, tr.output_missing, palette.warning),
        Some(DirProblem::NotADirectory) => hint(ui, tr.output_not_dir, palette.danger),
        Some(DirProblem::NotWritable) => hint(ui, tr.output_not_writable, palette.danger),
    }
}

/// El formato que se va a bajar. Se elige en la ficha cuando hay un enlace,
/// pero tambien es un ajuste: el que queda es el que arranca la proxima vez.
fn format(ui: &mut egui::Ui, app: &mut App, palette: &crate::palette::Palette) {
    let tr = app.tr();
    section(ui, tr.section_format, palette);

    let elegido = app.settings.format();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for opcion in crate::backend::FORMATS {
            let activo = opcion.id == elegido.id;
            if chip(
                ui,
                tr.format_label(opcion.id, opcion.label),
                activo,
                palette,
            )
            .clicked()
            {
                app.select_format(opcion.id);
            }
        }
    });

    hint(
        ui,
        &format!(
            "{} · {}",
            match elegido.kind {
                crate::backend::Kind::Video => tr.format_video,
                crate::backend::Kind::Audio => tr.format_audio_only,
            },
            elegido.args.join(" ")
        ),
        palette.dim,
    );
}

/// El `-o` de yt-dlp, para quien lo quiera tocar.
fn filename(ui: &mut egui::Ui, app: &mut App, palette: &crate::palette::Palette) {
    let tr = app.tr();
    section(ui, tr.section_filename, palette);

    let mut nuevo_template = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let presets = [
            (tr.filename_standard, crate::backend::DEFAULT_TEMPLATE),
            (tr.filename_uploader, "%(uploader)s - %(title)s.%(ext)s"),
            (
                tr.filename_numbered,
                "%(playlist_index)02d - %(title)s.%(ext)s",
            ),
            (tr.filename_date, "%(upload_date)s - %(title)s.%(ext)s"),
        ];
        for (label, tmpl) in presets {
            let activo = app.draft.filename_template.trim() == tmpl;
            if chip(ui, label, activo, palette).clicked() {
                nuevo_template = Some(tmpl.to_string());
            }
        }
    });
    if let Some(tmpl) = nuevo_template {
        app.draft.filename_template = tmpl;
        commit(app);
    }

    ui.add_space(8.0);
    let field = ui.add(
        egui::TextEdit::singleline(&mut app.draft.filename_template)
            .hint_text(text(
                crate::backend::DEFAULT_TEMPLATE,
                12.0,
                Weight::Regular,
                palette.dim,
            ))
            .font(Weight::Regular.font_id(12.0))
            .margin(egui::Margin::symmetric(12, 8))
            .desired_width(FIELD_WIDTH),
    );
    if field.lost_focus() {
        commit(app);
    }

    hint(ui, tr.filename_hint, palette.dim);
}

/// Limite maximo de velocidad de bajada para yt-dlp (--limit-rate).
fn rate_limit(ui: &mut egui::Ui, app: &mut App, palette: &crate::palette::Palette) {
    let tr = app.tr();
    section(ui, tr.section_rate, palette);

    let mut nuevo_limite = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let presets = [
            ("", tr.rate_unlimited),
            ("1M", "1 MB/s"),
            ("2M", "2 MB/s"),
            ("5M", "5 MB/s"),
            ("10M", "10 MB/s"),
        ];
        for (valor, etiqueta) in presets {
            let activo = app.draft.rate_limit.trim() == valor;
            if chip(ui, etiqueta, activo, palette).clicked() {
                nuevo_limite = Some(valor.to_string());
            }
        }
    });
    if let Some(limite) = nuevo_limite {
        app.draft.rate_limit = limite;
        commit(app);
    }

    ui.add_space(8.0);
    let field = ui.add(
        egui::TextEdit::singleline(&mut app.draft.rate_limit)
            .hint_text(text(
                tr.rate_custom_hint,
                12.0,
                Weight::Regular,
                palette.dim,
            ))
            .font(Weight::Regular.font_id(12.0))
            .margin(egui::Margin::symmetric(12, 8))
            .desired_width(FIELD_WIDTH),
    );
    if field.lost_focus() {
        commit(app);
    }

    hint(ui, tr.rate_hint, palette.dim);
}

/// Los idiomas de subtitulos. Antes eran "es" fijo; ahora los comunes son un
/// toque y los demas se escriben a mano.
fn subtitles(ui: &mut egui::Ui, app: &mut App, palette: &crate::palette::Palette) {
    let tr = app.tr();
    section(ui, tr.section_subtitles, palette);

    // Los que cubren casi todo lo que se baja. El resto, a mano.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (codigo, nombre) in [
            ("es", tr.lang_spanish),
            ("en", tr.lang_english),
            ("pt", tr.lang_portuguese),
            ("fr", tr.lang_french),
        ] {
            let activo = app.settings.subtitles.iter().any(|idioma| idioma == codigo);
            if chip(ui, codigo, activo, palette)
                .on_hover_text(nombre)
                .clicked()
            {
                app.toggle_subtitle(codigo);
            }
        }
    });

    ui.add_space(8.0);
    let field = ui.add(
        egui::TextEdit::singleline(&mut app.draft_subtitles)
            .hint_text(text("es, en", 12.0, Weight::Regular, palette.dim))
            .font(Weight::Regular.font_id(12.0))
            .margin(egui::Margin::symmetric(12, 8))
            .desired_width(FIELD_WIDTH),
    );
    if field.lost_focus() {
        commit(app);
    }

    match app.settings.subtitle_languages() {
        Some(idiomas) => hint(ui, &tr.subtitles_on(&idiomas), palette.dim),
        None => hint(ui, tr.subtitles_off, palette.dim),
    }
}

/// Las cookies del navegador, que es el issue de yoinks que el README
/// promete resolver. La lista sale de las carpetas que hay en la maquina.
fn cookies(ui: &mut egui::Ui, app: &mut App, palette: &crate::palette::Palette) {
    let tr = app.tr();
    section(ui, tr.section_cookies, palette);

    let detected = settings::browsers();
    let on = !app.settings.cookies_browser.trim().is_empty();
    let current = if on {
        app.settings.cookies_browser.clone()
    } else {
        app.default_browser.clone()
    };

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("reel-cookies-browser")
            .selected_text(text(&current, 12.0, Weight::Regular, palette.text))
            .width(200.0)
            .show_ui(ui, |ui| {
                // Elegir uno ya lo enciende: es lo que se quiso decir.
                for name in &detected {
                    let selected = on && *name == current;
                    if ui.selectable_label(selected, name).clicked() {
                        app.settings.cookies_browser = name.clone();
                        app.settings_changed();
                    }
                }
            });

        ui.add_space(10.0);
        if chip(ui, tr.use_cookies, on, palette).clicked() {
            if on {
                app.settings.cookies_browser.clear();
            } else {
                // Lo que el combo esta mostrando, que es lo que el usuario
                // tiene delante y no otro navegador elegido por detras.
                app.settings.cookies_browser = current.clone();
            }
            app.settings_changed();
        }
    });

    if detected.is_empty() {
        hint(ui, tr.no_browsers, palette.warning);
    } else {
        hint(ui, &tr.browsers_found(&detected.join(", ")), palette.dim);
    }
}

/// Opciones adicionales de descarga y posprocesado (SponsorBlock, capitulos, metadatos).
fn extras_setting(ui: &mut egui::Ui, app: &mut App, palette: &crate::palette::Palette) {
    let tr = app.tr();
    section(ui, tr.section_extras, palette);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        let sb_activo = app.draft.sponsorblock;
        if chip(ui, tr.sponsorblock_toggle, sb_activo, palette)
            .on_hover_text(tr.sponsorblock_hint)
            .clicked()
        {
            app.draft.sponsorblock = !sb_activo;
            commit(app);
        }

        let ch_activo = app.draft.chapters;
        if chip(ui, tr.chapters_toggle, ch_activo, palette)
            .on_hover_text(tr.chapters_hint)
            .clicked()
        {
            app.draft.chapters = !ch_activo;
            commit(app);
        }

        let meta_activo = app.draft.metadata;
        if chip(ui, tr.metadata_toggle, meta_activo, palette)
            .on_hover_text(tr.metadata_hint)
            .clicked()
        {
            app.draft.metadata = !meta_activo;
            commit(app);
        }
    });

    hint(ui, tr.sponsorblock_hint, palette.dim);
}

/// Argumentos adicionales que se le pasan directamente a yt-dlp.
fn advanced_setting(ui: &mut egui::Ui, app: &mut App, palette: &crate::palette::Palette) {
    let tr = app.tr();
    section(ui, tr.section_advanced, palette);

    let field = ui.add(
        egui::TextEdit::singleline(&mut app.draft.extra_args)
            .hint_text(text(
                "--proxy socks5://127.0.0.1:9050",
                12.0,
                Weight::Regular,
                palette.dim,
            ))
            .font(Weight::Regular.font_id(12.0))
            .margin(egui::Margin::symmetric(12, 8))
            .desired_width(FIELD_WIDTH),
    );
    if field.lost_focus() {
        commit(app);
    }

    hint(ui, tr.extra_args_hint, palette.dim);
}

/// El selector de tema: seguir el escritorio, o una paleta concreta. La
/// eleccion se persiste, que era lo otro que faltaba.
fn themes(ui: &mut egui::Ui, app: &mut App, palette: &crate::palette::Palette) {
    let tr = app.tr();
    section(ui, tr.section_theme, palette);

    let following = app.selected_theme.is_none();
    if chip(ui, tr.follow_desktop, following, palette).clicked() {
        app.select_theme(None);
        return;
    }
    ui.add_space(10.0);

    let catalog: Vec<(String, crate::palette::Palette)> = app
        .themes
        .picker_themes()
        .map(|theme| (theme.filename.clone(), theme.palette))
        .collect();

    let selected = app.selected_theme.clone();
    let mut pick: Option<Option<String>> = None;

    egui::Grid::new("reel-themes")
        .num_columns(THEMES_PER_ROW)
        .spacing(Vec2::new(8.0, 8.0))
        .show(ui, |ui| {
            for (index, (filename, swatch)) in catalog.iter().enumerate() {
                if swatch_button(ui, filename, *swatch, selected.as_deref(), palette) {
                    pick = Some(Some(filename.clone()));
                }
                if (index + 1) % THEMES_PER_ROW == 0 {
                    ui.end_row();
                }
            }
        });

    if let Some(choice) = pick {
        app.select_theme(choice);
    }
}

/// Una muestra de paleta como pastilla: los colores reales del tema, su
/// nombre, y el tilde si es el elegido.
fn swatch_button(
    ui: &mut egui::Ui,
    filename: &str,
    swatch: crate::palette::Palette,
    selected: Option<&str>,
    palette: &crate::palette::Palette,
) -> bool {
    let is_selected = selected == Some(filename);
    let label = fastframe_theme::display_name(filename);
    let size = Vec2::new(142.0, 34.0);

    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if !ui.is_rect_visible(rect) {
        return false;
    }

    let fill = if response.hovered() {
        palette.surface_hover
    } else {
        palette.surface
    };
    let stroke = if is_selected {
        Stroke::new(1.0, palette.accent)
    } else {
        Stroke::new(1.0, palette.outline)
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(8),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );

    // Tres cuadraditos: el fondo del tema, su acento y su texto.
    let dot = 10.0;
    let left = rect.left() + 10.0;
    let top = rect.center().y - dot / 2.0;
    for (index, color) in [swatch.window, swatch.accent, swatch.text]
        .iter()
        .enumerate()
    {
        let at = egui::Rect::from_min_size(
            egui::pos2(left + index as f32 * (dot + 3.0), top),
            Vec2::splat(dot),
        );
        ui.painter().rect_filled(at, CornerRadius::same(3), *color);
    }

    let text_left = left + 3.0 * (dot + 3.0) + 4.0;
    let color = if is_selected {
        palette.text
    } else {
        palette.secondary
    };
    ui.painter().text(
        egui::pos2(text_left, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        Weight::Medium.font_id(11.0),
        color,
    );

    if is_selected {
        let mark = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 15.0, rect.center().y),
            Vec2::splat(13.0),
        );
        egui::Image::new(crate::icon::Icon::Check.uri())
            .tint(palette.accent)
            .paint_at(ui, mark);
    }

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}
