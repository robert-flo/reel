//! El estado de la app y su frame.
//!
//! La app implementa `fastframe_shell::Resident`, asi que la ventana puede
//! cerrarse sin matar el proceso: la cola sigue bajando desde el tray y la
//! ventana vuelve cuando alguien la pide.

use std::sync::mpsc::{Receiver, Sender};

use fastframe_theme::{Catalog, DesktopThemes, Transition, Waker as ThemeWaker};

use crate::backend::{Backend, Command, Media, Options};
use crate::palette::Palette;
use crate::ui;

pub const SLUG: &str = "reel";
pub const APP_NAME: &str = "reel";

#[derive(Clone, Debug)]
pub enum UpdateState {
    Idle,
    Checking,
    Available { version: String },
    Downloading { received: u64, total: u64 },
    Ready,
    Unsupported(String),
    Failed(String),
}

/// Lo que el hilo de actualizacion le cuenta a la interfaz.
#[derive(Debug)]
pub enum UpdateMessage {
    State(UpdateState),
}

pub struct App {
    pub url: String,
    pub options: Options,
    pub preview: Option<Media>,
    pub backend: Backend,

    pub palette: Palette,
    pub wanted_palette: Palette,
    pub themes: Catalog<Palette>,
    pub transition: Transition,
    pub selected_theme: Option<String>,
    theme_waker: ThemeWaker,

    pub update: UpdateState,
    update_rx: Receiver<UpdateMessage>,
    update_tx: Sender<UpdateMessage>,

    pub tray: Option<fastframe_tray::Tray>,
    pub theme_picker_open: bool,
    pub settings_open: bool,
    pub paste_requested: bool,
    pub default_browser: String,

    // Lo que fastframe-shell necesita saber.
    hide_intent: bool,
    wants_show: bool,
    quit_requested: bool,
}

impl App {
    pub fn new(waker: &fastframe_shell::Waker) -> Self {
        let backend = {
            let waker = waker.clone();
            Backend::spawn(move || waker.wake())
        };

        let (update_tx, update_rx) = std::sync::mpsc::channel();

        let tray = {
            let waker = waker.clone();
            fastframe_tray::Tray::spawn(
                fastframe_tray::Config {
                    id: SLUG,
                    title: APP_NAME.into(),
                    icon: crate::icon::app_icon_rgba,
                    template_icon: Some(crate::icon::tray_template_rgba),
                    menu: vec![
                        fastframe_tray::MenuItem::action("show", "Mostrar reel"),
                        fastframe_tray::MenuItem::Separator,
                        fastframe_tray::MenuItem::action("paste", "Pegar y descargar"),
                        fastframe_tray::MenuItem::Separator,
                        fastframe_tray::MenuItem::action("quit", "Salir"),
                    ],
                },
                move || waker.wake(),
            )
        };

        let palette = Palette::dark();
        let theme_waker = {
            let waker = waker.clone();
            ThemeWaker::new(move || waker.wake())
        };

        Self {
            url: String::new(),
            options: Options::default(),
            preview: None,
            backend,
            palette,
            wanted_palette: palette,
            themes: Catalog::default(),
            transition: Transition::default(),
            selected_theme: None,
            theme_waker,
            update: UpdateState::Idle,
            update_rx,
            update_tx,
            tray,
            theme_picker_open: false,
            settings_open: false,
            paste_requested: false,
            default_browser: "firefox".into(),
            hide_intent: false,
            wants_show: false,
            quit_requested: false,
        }
    }

    /// Arranca el catalogo de temas: los archivos del usuario, las ocho
    /// paletas compartidas, y el tema de Omarchy en vivo cuando el escritorio
    /// esta ahi.
    pub fn start_themes(&mut self) {
        self.themes.enable_desktop_themes(DesktopThemes {
            slug: SLUG,
            omarchy_template: include_str!("../contrib/omarchy/reel.json.tpl"),
            omarchy_previous_templates: &[],
            presets: true,
        });

        self.rescan_themes();
    }

    /// Pide al catalogo que liste el directorio otra vez. El resultado llega
    /// por `poll` en el siguiente frame.
    fn rescan_themes(&mut self) {
        let waker = self.theme_waker.clone();
        self.themes.start(
            crate::dirs::config_dir().join("themes"),
            self.selected_theme.clone(),
            &waker,
        );
    }

    pub fn attach(&mut self, ctx: &egui::Context) {
        crate::fonts::setup(ctx);
        egui_extras::install_image_loaders(ctx);
        crate::icon::install(ctx);
        self.apply_palette(ctx, self.palette);
        if let Some(tray) = &mut self.tray {
            tray.attach();
        }
    }

    fn apply_palette(&mut self, ctx: &egui::Context, palette: Palette) {
        self.palette = palette;
        ctx.set_visuals(palette.visuals());
        // El ajuste de hinting de fastframe-text se vuelve a aplicar sobre
        // los visuals nuevos, en los dos temas.
        let rendering = fastframe_text::detect();
        ctx.all_styles_mut(|style| rendering.apply_to_visuals(&mut style.visuals));
    }

    /// Lo que no dibuja: eventos, temas, tray, actualizador. Corre antes de
    /// cada frame y tambien sin ventana abierta.
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.pump(ctx);
    }

    /// El frame. En egui 0.36 la app recibe el `Ui` raiz y los paneles se
    /// muestran dentro de el: primero los de los bordes, el central de
    /// ultimo.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui::top_bar::show(self, ui);
        ui::status_bar::show(self, ui);

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.palette.window).inner_margin(
                egui::Margin::symmetric(ui::Metrics::GUTTER as i8, ui::Metrics::GUTTER as i8),
            ))
            .show(ui, |cui| {
                ui::url_bar::show(self, cui);
                cui.add_space(ui::Metrics::GAP);
                if self.preview.is_some() {
                    ui::media_card::show(self, cui);
                    cui.add_space(ui::Metrics::GAP + 6.0);
                }
                ui::queue::show(self, cui);
            });

        // La revelacion de colores desde el centro, como hace Omarchy. Va de
        // ultimo en el frame, siempre.
        self.transition.paint(ui.ctx());
    }

    /// Lo que corre con o sin ventana: eventos del backend, del tray, del
    /// actualizador y del catalogo de temas.
    pub fn background_frame(&mut self, ctx: &egui::Context) {
        self.pump(ctx);
    }

    fn pump(&mut self, ctx: &egui::Context) {
        if self.backend.drain() {
            self.sync_preview();
            ctx.request_repaint();
        }

        for message in self.update_rx.try_iter() {
            let UpdateMessage::State(state) = message;
            self.update = state;
        }

        if self.themes.needs_reload() {
            self.reload_themes();
        }
        if self.themes.poll() {
            self.resolve_palette();
        }

        if self.wanted_palette != self.palette {
            self.transition.begin(ctx);
            if !self.transition.holding(ctx) {
                let wanted = self.wanted_palette;
                self.apply_palette(ctx, wanted);
            }
        }

        self.pump_tray();
    }

    fn pump_tray(&mut self) {
        let Some(tray) = &self.tray else { return };
        let mut show = false;
        let mut paste = false;
        let mut quit = false;

        for event in tray.events() {
            match event {
                fastframe_tray::Event::Toggle | fastframe_tray::Event::Menu("show") => show = true,
                fastframe_tray::Event::Show => show = true,
                fastframe_tray::Event::Menu("paste") => {
                    show = true;
                    paste = true;
                }
                fastframe_tray::Event::Menu("quit") => quit = true,
                fastframe_tray::Event::Menu(_) => {}
            }
        }

        if show {
            self.wants_show = true;
            self.hide_intent = false;
        }
        if paste {
            self.paste_requested = true;
        }
        if quit {
            self.quit_requested = true;
        }
    }

    /// El catalogo avisa con `needs_reload` que un archivo cambio; el escaneo
    /// se pide con `start` y sus resultados llegan por `poll`.
    fn reload_themes(&mut self) {
        self.rescan_themes();
    }

    /// El tema elegido por nombre de archivo; si no hay eleccion, el del
    /// escritorio; si tampoco, el oscuro de la app.
    fn resolve_palette(&mut self) {
        let chosen = self
            .selected_theme
            .as_deref()
            .and_then(|filename| self.themes.find(filename))
            .or_else(|| self.themes.system_theme());

        self.wanted_palette = chosen
            .map(|theme| theme.palette)
            .unwrap_or_else(Palette::dark);
    }

    fn sync_preview(&mut self) {
        let queue = self
            .backend
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(job) = queue.jobs.last() {
            if !job.media.title.is_empty() {
                self.preview = Some(job.media.clone());
            }
        }
    }

    pub fn enqueue_current_url(&mut self) {
        let url = self.url.trim().to_string();
        if url.is_empty() {
            return;
        }

        let id = {
            let mut queue = self
                .backend
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            queue.push(url.clone(), self.options.clone())
        };

        self.backend.send(Command::Probe {
            id,
            url: url.clone(),
        });
        self.backend.send(Command::Start {
            id,
            url,
            options: self.options.clone(),
        });
        self.url.clear();
    }

    pub fn clipboard_text(&self, _ctx: &egui::Context) -> Option<String> {
        // egui entrega el portapapeles por eventos; en Wayland tambien vale
        // `wl-paste`. Un solo lugar que cambiar cuando se decida cual.
        std::process::Command::new("wl-paste")
            .output()
            .ok()
            .filter(|out| out.status.success())
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    }

    pub fn output_dir_label(&self) -> String {
        match &self.options.output_dir {
            Some(dir) => dir.display().to_string(),
            None => {
                let kind = crate::backend::format_by_id(&self.options.format_id).kind;
                match kind {
                    crate::backend::Kind::Audio => "~/Music".into(),
                    crate::backend::Kind::Video => "~/Videos".into(),
                }
            }
        }
    }

    pub fn theme_label(&self) -> String {
        if self.themes.follows_omarchy() && self.selected_theme.is_none() {
            match self.themes.system_theme() {
                Some(theme) => format!(
                    "tema: siguiendo omarchy ({})",
                    fastframe_theme::display_name(&theme.filename)
                ),
                None => "tema: siguiendo omarchy".into(),
            }
        } else {
            match &self.selected_theme {
                Some(name) => format!("tema: {name}"),
                None => "tema: por defecto".into(),
            }
        }
    }

    pub fn start_update_download(&mut self) {
        crate::updates::download(self.update_tx.clone());
    }

    pub fn apply_update(&mut self) {
        crate::updates::handoff();
        self.quit_requested = true;
    }

    pub fn check_updates(&self) {
        crate::updates::check(self.update_tx.clone());
    }

    pub fn hides_to_tray(&self) -> bool {
        self.tray.is_some()
    }
}

// --- fastframe-shell -------------------------------------------------------

impl fastframe_shell::Resident for App {
    fn closed(&self) -> fastframe_shell::Closed {
        if !self.quit_requested && self.hide_intent {
            fastframe_shell::Closed::Hide
        } else {
            fastframe_shell::Closed::Quit
        }
    }

    fn window_gone(&mut self) {
        self.hide_intent = false;
        self.wants_show = false;
        self.theme_picker_open = false;
        self.settings_open = false;
    }

    fn headless_frame(&mut self, ctx: &egui::Context) -> fastframe_shell::Headless {
        self.background_frame(ctx);
        if self.quit_requested {
            fastframe_shell::Headless::Quit
        } else if self.wants_show {
            fastframe_shell::Headless::Show
        } else {
            fastframe_shell::Headless::Wait
        }
    }

    fn start_hidden(&mut self) -> bool {
        if !self.hides_to_tray() {
            return false;
        }
        self.hide_intent = true;
        true
    }

    fn shutdown(&mut self) {
        self.backend.send(Command::Shutdown);
    }
}
