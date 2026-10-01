//! El estado de la app y su frame.
//!
//! La app implementa `fastframe_shell::Resident`, asi que la ventana puede
//! cerrarse sin matar el proceso: la cola sigue bajando desde el tray y la
//! ventana vuelve cuando alguien la pide.

use std::sync::mpsc::{Receiver, Sender};

use fastframe_theme::{Catalog, DesktopThemes, Transition, Waker as ThemeWaker};

use crate::backend::{Backend, Command, Media, Options};
use crate::palette::Palette;
use crate::settings;
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
    /// Lo que contesto `yt-dlp --version`.
    Ytdlp(Result<String, String>),
}

pub struct App {
    pub url: String,
    /// El formato elegido para el proximo trabajo. Lo demas sale de `settings`.
    pub options: Options,
    /// Lo que dura entre arranques: carpeta, plantilla, cookies y tema.
    pub settings: settings::Settings,
    /// Copia de trabajo de los campos de texto del panel, para no validar una
    /// ruta a medio tipear. Se confirma al cerrar el panel.
    pub draft: settings::Settings,
    /// Los idiomas de subtitulos mientras se escriben: `es, en`.
    pub draft_subtitles: String,
    /// El panel de ajustes esta abierto.
    pub settings_open: bool,
    /// Algo de `settings` cambio y hay que escribirlo a disco.
    settings_dirty: bool,
    pub preview: Option<Media>,
    /// El enlace al que corresponde la ficha de arriba.
    pub preview_url: Option<String>,
    pub preview_error: Option<String>,
    /// Hay un vistazo en curso: el boton dice "leyendo".
    pub probing: bool,
    /// Lo pidio "Pegar y descargar" del tray: cuando el vistazo llegue, el
    /// trabajo entra a la cola solo, sin pasar por el boton.
    enqueue_when_probed: bool,
    /// Que dijo `yt-dlp --version`, o por que no se puede usar. Se averigua en
    /// un hilo al arrancar para poder avisar antes de que alguien apriete
    /// "descargar" y se coma el error del proceso.
    pub ytdlp: Option<Result<String, String>>,
    /// Una lista grande quedo esperando que se confirme. El primer toque en el
    /// boton solo pregunta; el segundo encola. Una lista de 19 videos de
    /// YouTube son gigabytes, y encolarla por error cuesta ancho de banda y
    /// disco, no un clic.
    pub confirmar_lista: Option<u64>,
    /// Cuantos trabajos habia activos en el frame anterior. Sirve para avisar
    /// cuando la cola pasa de tener trabajo a estar quieta, y no en cada
    /// archivo: encolar diez avisaria diez veces.
    activos_antes: usize,
    /// El tray pidio pegar; se atiende en el hilo de la interfaz, que es donde
    /// egui y `wl-paste` se llevan bien.
    pub paste_requested: bool,
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

        // Lo elegido la ultima vez: el tema arranca de ahi y no del default,
        // que era justo lo que se perdia al cerrar.
        let settings = settings::Settings::load();
        let default_browser = if settings.cookies_browser.trim().is_empty() {
            settings::detect_browser()
        } else {
            settings.cookies_browser.clone()
        };

        let mut app = Self {
            url: String::new(),
            options: Options::default(),
            settings: settings.clone(),
            draft: settings.clone(),
            draft_subtitles: settings.subtitle_list(),
            settings_open: false,
            settings_dirty: false,
            preview: None,
            preview_url: None,
            preview_error: None,
            probing: false,
            enqueue_when_probed: false,
            confirmar_lista: None,
            ytdlp: None,
            activos_antes: 0,
            paste_requested: false,
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
            default_browser,
            hide_intent: false,
            wants_show: false,
            quit_requested: false,
        };

        app.selected_theme = app.settings.theme.clone();
        app.sync_options_from_settings();
        app
    }

    /// El formato lo elige la ficha; lo demas son ajustes, y viven en un solo
    /// lugar para que el panel y lo que se le pasa a yt-dlp no se separen.
    /// Se llama al arrancar y cada vez que el panel toca algo.
    pub fn sync_options_from_settings(&mut self) {
        // El formato tambien es un ajuste: el que se eligio la ultima vez es el
        // que arranca. `Settings::format` cae al primero si el guardado ya no
        // existe.
        self.options.format_id = self.settings.format().id.to_string();
        self.options.output_dir = self.settings.output_path();
        // `template` resuelve el vacio; `None` deja que yt-dlp use su default.
        self.options.filename_template = {
            let trimmed = self.settings.filename_template.trim();
            (!trimmed.is_empty()).then(|| self.settings.template().to_string())
        };
        self.options.cookies_from_browser = {
            let browser = self.settings.cookies_browser.trim();
            (!browser.is_empty()).then(|| browser.to_string())
        };
        self.options.subtitles = self.settings.subtitle_languages();
    }

    /// Abre el panel con una copia fresca de lo guardado, para que un borrador
    /// viejo no reviva al reabrirlo.
    pub fn open_settings(&mut self) {
        self.draft = self.settings.clone();
        self.draft_subtitles = self.settings.subtitle_list();
        self.settings_open = true;
    }

    /// Poner o quitar un idioma desde la ficha, para que el panel y la ficha no
    /// se contradigan.
    pub fn toggle_subtitle(&mut self, idioma: &str) {
        let mut idiomas = self.settings.subtitles.clone();
        match idiomas.iter().position(|ya| ya == idioma) {
            Some(at) => {
                idiomas.remove(at);
            }
            None => idiomas.push(idioma.to_string()),
        }
        self.settings.subtitles = idiomas;
        self.draft_subtitles = self.settings.subtitle_list();
        self.settings_changed();
    }

    /// Marca los ajustes como cambiados. Lo que corre sin ventana los escribe
    /// en el siguiente frame, en un hilo aparte: guardar no frena la interfaz.
    pub fn settings_changed(&mut self) {
        self.sync_options_from_settings();
        self.settings_dirty = true;
    }

    /// Elegir formato se guarda, para no volver a "Mejor" en cada arranque.
    pub fn select_format(&mut self, format_id: &str) {
        self.settings.format_id = format_id.to_string();
        self.settings_changed();
    }

    /// Elegir tema es un ajuste mas, asi que se guarda como cualquier otro.
    pub fn select_theme(&mut self, filename: Option<String>) {
        self.selected_theme = filename.clone();
        self.settings.theme = filename;
        self.settings_dirty = true;
        self.rescan_themes();
    }

    /// Escribe lo que haya pendiente una sola vez por frame.
    fn save_settings(&mut self) {
        if !std::mem::replace(&mut self.settings_dirty, false) {
            return;
        }
        let settings = self.settings.clone();
        std::thread::spawn(move || {
            if let Err(error) = settings.save() {
                log::warn!("no pude guardar los ajustes: {error}");
            }
        });
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

        // El modal va despues del panel central: encima de todo, con su velo
        // detras, y comiendose el Escape y los clics de afuera.
        if self.settings_open && ui::settings::show(self, ui.ctx()) {
            self.settings_open = false;
        }

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
        let (cambio, nuevos) = self.backend.drain();
        if cambio {
            self.sync_preview();
            ctx.request_repaint();
        }
        // Los videos de una lista se encolaron al leerla; ahora hay que
        // mandarlos a bajar, que es lo que la cola no puede hacer sola.
        for id in nuevos {
            if let Some((url, options)) = self.job_data(id) {
                self.backend.send(Command::Start { id, url, options });
            }
        }

        self.avisar_si_termino();
        self.pump_paste();

        for message in self.update_rx.try_iter() {
            match message {
                UpdateMessage::State(state) => self.update = state,
                UpdateMessage::Ytdlp(resultado) => self.ytdlp = Some(resultado),
            }
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
        self.save_settings();
    }

    /// Avisa por el escritorio cuando la cola deja de tener trabajo. Se mira
    /// el cambio y no cada trabajo: al encolar varios, avisar por archivo
    /// seria una lluvia de notificaciones justo cuando el usuario esta mirando
    /// la ventana.
    fn avisar_si_termino(&mut self) {
        let (activos, hechos, fallados) = {
            let queue = self
                .backend
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let activos = queue.active();
            let hechos = queue.done();
            let fallados = queue
                .jobs
                .iter()
                .filter(|job| matches!(job.state, crate::backend::State::Failed { .. }))
                .count();
            (activos, hechos, fallados)
        };

        let antes = std::mem::replace(&mut self.activos_antes, activos);
        if activos > 0 || antes == 0 {
            return;
        }

        // Si nada salio bien, decirlo tambien tiene valor.
        let cuerpo = match (hechos, fallados) {
            (0, n) => format!(
                "{n} {} fallo",
                if n == 1 { "descarga" } else { "descargas" }
            ),
            (n, 0) => format!(
                "{n} {} listo",
                if n == 1 { "descarga" } else { "descargas" }
            ),
            (bien, mal) => format!("{bien} listas, {mal} con error"),
        };
        avisar("reel", &cuerpo);
    }

    /// Cuantos videos tiene que traer una lista para pedir confirmacion. Con
    /// menos, encolarla es un gesto barato y preguntar solo molesta.
    pub const LISTA_GRANDE: u64 = 10;

    /// Decide si hay que preguntar antes de encolar una lista.
    pub fn pide_confirmacion(cuantos: u64) -> bool {
        cuantos >= Self::LISTA_GRANDE
    }

    /// Lee un enlace y lo encola solo, sin pasar por el boton. Es lo que usan
    /// "Pegar y descargar" del tray y `reel --yoink URL`: el mismo camino, con
    /// el enlace viniendo de otro lado.
    pub fn yoink(&mut self, url: String) {
        if url.trim().is_empty() {
            return;
        }
        self.url = url;
        self.enqueue_when_probed = true;
        // Si el panel estaba abierto, se cierra: el enlace viene a la cola, no
        // a que alguien lo mire.
        self.settings_open = false;
        self.preview_current_url();
    }

    /// "Pegar y descargar" del tray: pega, lee el enlace y deja marcado que el
    /// trabajo entre a la cola solo. Antes esto solo levantaba una bandera que
    /// nadie miraba.
    fn pump_paste(&mut self) {
        if !std::mem::replace(&mut self.paste_requested, false) {
            return;
        }
        let Some(clipped) = self.clipboard_text_uncached() else {
            log::warn!("no habia nada para pegar en el portapapeles");
            return;
        };
        self.yoink(clipped);
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
        let (preview, error) = {
            let queue = self
                .backend
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (queue.preview.clone(), queue.preview_error.clone())
        };

        if let Some((url, media)) = preview {
            self.preview = Some(media);
            self.preview_url = Some(url);
            self.preview_error = None;
            self.probing = false;

            // Lo pidio el tray: no hay nadie para apretar "descargar".
            if std::mem::take(&mut self.enqueue_when_probed) {
                self.enqueue_preview();
            }
        }
        if let Some(reason) = error {
            self.preview = None;
            self.preview_url = None;
            self.preview_error = Some(reason);
            self.probing = false;
            self.enqueue_when_probed = false;
        }
    }

    /// Paso uno: leer el enlace y pintar la ficha. No descarga nada todavia,
    /// que es justo el punto de tener formatos que elegir.
    pub fn preview_current_url(&mut self) {
        let url = self.url.trim().to_string();
        if url.is_empty() {
            return;
        }

        {
            let mut queue = self
                .backend
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            queue.preview = None;
            queue.preview_error = None;
        }

        self.preview = None;
        self.preview_url = None;
        self.preview_error = None;
        self.probing = true;
        // La lista que estaba por confirmarse ya no es la que se va a encolar.
        self.confirmar_lista = None;
        self.backend.send(Command::Preview { url });
    }

    /// Paso dos: con el formato ya elegido, a descargar. La ficha se queda
    /// puesta para poder encolar otra calidad del mismo enlace.
    pub fn enqueue_preview(&mut self) {
        let (Some(url), Some(media)) = (self.preview_url.clone(), self.preview.clone()) else {
            return;
        };

        let es_lista = media.playlist_count.is_some();
        let mut options = self.options.clone();
        options.playlist = es_lista;

        let id = {
            let mut queue = self
                .backend
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            queue.push_ready(url.clone(), options.clone(), media)
        };

        if es_lista {
            // Una lista no se baja como un trabajo: primero hay que saber que
            // videos trae, y esa lectura la hace el worker.
            self.backend.send(Command::Expandir { id, url });
        } else {
            self.backend.send(Command::Start { id, url, options });
        }
        self.url.clear();
    }

    /// La url y las opciones de un trabajo, para poder volver a pedirlo.
    fn job_data(&self, id: u64) -> Option<(String, Options)> {
        let queue = self
            .backend
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queue
            .jobs
            .iter()
            .find(|job| job.id == id)
            .map(|job| (job.url.clone(), job.options.clone()))
    }

    /// Vuelve a encolar un trabajo terminado. Los mismos argumentos que la
    /// primera vez, que es lo que hace que yt-dlp reanude el `.part` que quedo
    /// en la carpeta en vez de empezar de cero.
    pub fn retry(&mut self, id: u64) -> bool {
        let pedido = {
            let mut queue = self
                .backend
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            queue.retry(id)
        };
        let Some((url, options)) = pedido else {
            return false;
        };
        self.backend.send(Command::Start { id, url, options });
        true
    }

    /// Reintenta todo lo que no este andando. Sirve cuando se cae la red y
    /// fallan varios de una: no hay que ir uno por uno.
    pub fn retry_failed(&mut self) -> usize {
        let pedidos: Vec<(u64, String, Options)> = {
            let mut queue = self
                .backend
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let ids: Vec<u64> = queue
                .jobs
                .iter()
                .filter(|job| !job.is_active())
                .map(|job| job.id)
                .collect();
            ids.into_iter()
                .filter_map(|id| queue.retry(id).map(|(url, options)| (id, url, options)))
                .collect()
        };

        let cuantos = pedidos.len();
        for (id, url, options) in pedidos {
            self.backend.send(Command::Start { id, url, options });
        }
        cuantos
    }

    pub fn clipboard_text(&self, _ctx: &egui::Context) -> Option<String> {
        // egui entrega el portapapeles por eventos; en Wayland tambien vale
        // `wl-paste`. Un solo lugar que cambiar cuando se decida cual.
        self.clipboard_text_uncached()
    }

    /// Sin `Context`, para el camino del tray, que no tiene frame a mano.
    fn clipboard_text_uncached(&self) -> Option<String> {
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

    /// Pregunta por yt-dlp en un hilo: arrancar la ventana no puede depender de
    /// lanzar un proceso.
    pub fn check_ytdlp(&self) {
        let tx = self.update_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(UpdateMessage::Ytdlp(crate::backend::ytdlp::version()));
        });
    }

    pub fn hides_to_tray(&self) -> bool {
        self.tray.is_some()
    }
}

/// Una notificacion del escritorio, con lo que haya. No es criticalo: si no
/// hay servidor de notificaciones, se pierde el aviso y la app sigue.
fn avisar(titulo: &str, cuerpo: &str) {
    if let Err(error) = std::process::Command::new("notify-send")
        .args(["--app-name=reel", "--icon=reel", titulo, cuerpo])
        .spawn()
    {
        log::debug!("no pude avisar por el escritorio: {error}");
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

#[cfg(test)]
mod tests {
    use super::App;

    /// Una lista chica se encola de un toque; una grande pide confirmar. El
    /// limite es lo unico que decide, asi que se prueba el borde.
    #[test]
    fn las_listas_grandes_piden_confirmacion() {
        assert!(!App::pide_confirmacion(0));
        assert!(!App::pide_confirmacion(1));
        assert!(!App::pide_confirmacion(App::LISTA_GRANDE - 1));
        assert!(App::pide_confirmacion(App::LISTA_GRANDE));
        assert!(App::pide_confirmacion(19));
        assert!(App::pide_confirmacion(500));
    }
}
