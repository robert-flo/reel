//! El estado de la app y su frame.
//!
//! La app implementa `fastframe_shell::Resident`, asi que la ventana puede
//! cerrarse sin matar el proceso: la cola sigue bajando desde el tray y la
//! ventana vuelve cuando alguien la pide.

use std::sync::mpsc::{Receiver, Sender};

use fastframe_theme::{Catalog, DesktopThemes, Transition, Waker as ThemeWaker};

use crate::backend::{Backend, Command, Media, Options};
use crate::i18n::Language;
use crate::palette::Palette;
use crate::settings;
use crate::ui;

pub const SLUG: &str = "reel";
pub const APP_NAME: &str = "reel";

#[derive(Clone, Debug)]
pub enum UpdateState {
    Idle,
    Checking,
    /// El repositorio todavia no tiene ninguna version publicada. No es una
    /// falla: es que no hay nada que ofrecer.
    SinReleases,
    Available {
        version: String,
    },
    Downloading {
        received: u64,
        total: u64,
    },
    Ready,
    Unsupported(String),
    Failed(String),
}

/// Como se le pregunta la version a una herramienta del sistema.
type Revisor = fn() -> Result<String, String>;

/// Lo que contestaron las herramientas externas, cada una `None` mientras su
/// hilo no haya respondido.
#[derive(Default)]
pub struct Herramientas {
    pub ytdlp: Option<Result<String, String>>,
    pub ffmpeg: Option<Result<String, String>>,
}

/// Lo que el hilo de actualizacion le cuenta a la interfaz.
#[derive(Debug)]
pub enum UpdateMessage {
    State(UpdateState),
    /// Lo que contesto la version de una herramienta: `true` para yt-dlp,
    /// `false` para ffmpeg.
    Version(bool, Result<String, String>),
    /// Lo que pidio otra instancia de la app.
    Aviso(crate::instancia::Aviso),
}

/// Filtro visible de la cola de descargas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum QueueFilter {
    #[default]
    All,
    Active,
    Done,
    Failed,
}

pub struct App {
    pub url: String,
    /// Filtro de la cola para ver todas, solo activas, terminadas o falladas.
    pub queue_filter: QueueFilter,
    /// Texto de busqueda para filtrar la cola por titulo o enlace.
    pub queue_search: String,
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
    /// Que contestaron las herramientas del sistema. Se averigua en un hilo al
    /// arrancar para poder avisar antes de que alguien apriete "descargar" y se
    /// coma el error del proceso.
    pub herramientas: Herramientas,
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

        // Lo elegido la ultima vez: el tema y el idioma arrancan de ahi y no
        // del default, que era justo lo que se perdia al cerrar.
        let settings = settings::Settings::load();
        let tr = settings.language.catalog();

        let tray = {
            let waker = waker.clone();
            fastframe_tray::Tray::spawn(
                fastframe_tray::Config {
                    id: SLUG,
                    title: APP_NAME.into(),
                    icon: crate::icon::app_icon_rgba,
                    template_icon: Some(crate::icon::tray_template_rgba),
                    menu: vec![
                        fastframe_tray::MenuItem::action("show", tr.tray_show),
                        fastframe_tray::MenuItem::Separator,
                        fastframe_tray::MenuItem::action("paste", tr.tray_paste),
                        fastframe_tray::MenuItem::Separator,
                        fastframe_tray::MenuItem::action("quit", tr.tray_quit),
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

        let default_browser = if settings.cookies_browser.trim().is_empty() {
            settings::detect_browser()
        } else {
            settings.cookies_browser.clone()
        };

        let mut app = Self {
            url: String::new(),
            queue_filter: QueueFilter::default(),
            queue_search: String::new(),
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
            herramientas: Herramientas::default(),
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

    /// Los textos de la interfaz en el idioma elegido.
    pub fn tr(&self) -> &'static crate::i18n::Catalog {
        self.settings.language.catalog()
    }

    /// Cambiar el idioma se aplica en el frame siguiente: egui redibuja con
    /// el catalogo nuevo, sin reiniciar. El menu del tray nacio con el idioma
    /// del arranque y se actualiza la proxima vez que se abre la app.
    pub fn select_language(&mut self, language: Language) {
        if self.settings.language == language {
            return;
        }
        self.settings.language = language;
        self.settings_changed();
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
        self.options.rate_limit = {
            let limit = self.settings.rate_limit.trim();
            (!limit.is_empty()).then(|| limit.to_string())
        };
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
        self.handle_shortcuts(ui);
        self.handle_dropped_files(ui);
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

    /// Atajos de teclado globales:
    /// - `Ctrl+,`: abrir/cerrar ajustes
    /// - `Ctrl+Q`: salir
    /// - `Ctrl+L`: enfocar el campo de enlace
    /// - `Ctrl+F`: enfocar el buscador de la cola
    /// - `Escape`: cerrar ajustes o limpiar url/vista previa/busqueda
    /// - `Ctrl+V` (sin foco en texto): pegar url y obtener vista previa
    fn handle_shortcuts(&mut self, ui: &egui::Ui) {
        let foco_en_texto = ui.memory(|m| m.focused().is_some());
        let mut focus_url = false;
        let mut focus_search = false;
        ui.input(|i| {
            if i.modifiers.command && i.key_pressed(egui::Key::Comma) {
                if self.settings_open {
                    self.settings_open = false;
                    self.save_settings();
                } else {
                    self.open_settings();
                }
            }

            if i.modifiers.command && i.key_pressed(egui::Key::Q) {
                self.quit_requested = true;
            }

            if !foco_en_texto && i.modifiers.command && i.key_pressed(egui::Key::L) {
                focus_url = true;
            }

            if !foco_en_texto && i.modifiers.command && i.key_pressed(egui::Key::F) {
                focus_search = true;
            }

            if i.key_pressed(egui::Key::Escape) {
                if self.settings_open {
                    self.settings_open = false;
                } else if self.preview.is_some() || self.preview_error.is_some() {
                    self.preview = None;
                    self.preview_url = None;
                    self.preview_error = None;
                    self.probing = false;
                    self.confirmar_lista = None;
                } else if !self.queue_search.is_empty() {
                    self.queue_search.clear();
                } else if !self.url.is_empty() {
                    self.url.clear();
                }
            }

            if !foco_en_texto && i.modifiers.command && i.key_pressed(egui::Key::V) {
                if let Some(clipped) = self.clipboard_text_uncached() {
                    self.url = clipped;
                    self.preview_current_url();
                }
            }
        });

        if focus_url {
            ui.ctx()
                .memory_mut(|m| m.request_focus(egui::Id::new("url_input")));
        }
        if focus_search {
            ui.ctx()
                .memory_mut(|m| m.request_focus(egui::Id::new("queue_search_input")));
        }
    }

    /// Importar enlaces al arrastrar archivos (como un .txt con URLs) hacia la ventana.
    fn handle_dropped_files(&mut self, ui: &egui::Ui) {
        let dropped = ui.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            let path = file.path();
            if !path.as_os_str().is_empty() {
                if let Ok(content) = std::fs::read_to_string(path) {
                    let urls: Vec<String> = content
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
                        .collect();
                    for url in urls {
                        self.enqueue_url_direct(url);
                    }
                    continue;
                }
            }
            if let Ok(bytes) = file.bytes() {
                if let Ok(content) = std::str::from_utf8(&bytes) {
                    let urls: Vec<String> = content
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
                        .collect();
                    for url in urls {
                        self.enqueue_url_direct(url);
                    }
                }
            }
        }
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

        // Se juntan primero y se aplican despues: `try_iter` presta `self`
        // mientras dura el bucle, y encolar un enlace lo pide prestado mutable.
        let mensajes: Vec<UpdateMessage> = self.update_rx.try_iter().collect();
        for message in mensajes {
            match message {
                UpdateMessage::State(state) => self.update = state,
                UpdateMessage::Version(es_ytdlp, resultado) => {
                    if es_ytdlp {
                        self.herramientas.ytdlp = Some(resultado);
                    } else {
                        self.herramientas.ffmpeg = Some(resultado);
                    }
                }
                UpdateMessage::Aviso(aviso) => match aviso {
                    // Otra copia de la app le paso el enlace a esta: se encola
                    // aca, y la otra se cierra sin abrir ventana.
                    crate::instancia::Aviso::Yoink(url) => self.yoink(url),
                    crate::instancia::Aviso::Mostrar => {
                        self.wants_show = true;
                        self.hide_intent = false;
                    }
                },
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
        let cuerpo = self.tr().notify_body(hechos, fallados);
        avisar("reel", &cuerpo);
    }

    /// Cuantos videos tiene que traer una lista para pedir confirmacion. Con
    /// menos, encolarla es un gesto barato y preguntar solo molesta.
    pub const LISTA_GRANDE: u64 = 10;
    /// 1 GB en bytes: si una lista pesa mas que esto, tambien pide confirmacion.
    pub const PESO_GRANDE: u64 = 1_000_000_000;

    /// Decide si hay que preguntar antes de encolar una lista.
    /// Pide confirmacion si trae 10 o mas videos, o si pesa 1 GB o mas.
    pub fn pide_confirmacion(cuantos: u64, peso: Option<u64>) -> bool {
        cuantos >= Self::LISTA_GRANDE || peso.is_some_and(|p| p >= Self::PESO_GRANDE)
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

    /// Encola un enlace directo sin necesidad de previsualizar primero.
    pub fn enqueue_url_direct(&mut self, url: String) {
        let trimmed = url.trim().to_string();
        if trimmed.is_empty() {
            return;
        }

        let options = self.options.clone();
        let media = Media {
            title: trimmed.clone(),
            ..Default::default()
        };

        let id = {
            let mut queue = self
                .backend
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            queue.push_ready(trimmed.clone(), options.clone(), media)
        };

        self.backend.send(Command::Start {
            id,
            url: trimmed,
            options,
        });
    }

    /// Paso uno: leer el enlace y pintar la ficha. Si se pegaron multiples
    /// enlaces a la vez (separados por lineas), se encolan todos directamente.
    pub fn preview_current_url(&mut self) {
        let lines: Vec<String> = self
            .url
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        if lines.is_empty() {
            return;
        }

        if lines.len() > 1 {
            for line in lines {
                self.enqueue_url_direct(line);
            }
            self.url.clear();
            self.preview = None;
            self.preview_url = None;
            self.preview_error = None;
            self.probing = false;
            return;
        }

        let url = lines[0].clone();

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

    /// Vuelve a bajar un trabajo de cero, aunque el archivo ya este. La salida
    /// para un archivo que quedo truncado: `retry` lo saltearia y diria "listo"
    /// sobre el mismo archivo roto.
    pub fn retry_forzado(&mut self, id: u64) {
        let pedido = {
            let mut queue = self
                .backend
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            queue.retry_forzado(id)
        };
        if let Some((url, options)) = pedido {
            self.backend.send(Command::Start { id, url, options });
        }
    }

    /// Reintenta todos los trabajos que fallaron o se cancelaron. No toca los
    /// que ya terminaron bien, evitando descargas redundantes.
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
                .filter(|job| {
                    matches!(
                        job.state,
                        crate::backend::State::Failed { .. } | crate::backend::State::Cancelled
                    )
                })
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

    /// Quita un trabajo de la cola si no esta activo.
    pub fn remove_job(&mut self, id: u64) -> bool {
        let mut queue = self
            .backend
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.remove(id)
    }

    /// Quita todos los trabajos terminados (listo, cancelado o fallo) de la cola.
    pub fn clear_finished_jobs(&mut self) -> usize {
        let mut queue = self
            .backend
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.clear_finished()
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

    /// Copia texto tanto al portapapeles de egui como al del sistema (Wayland / wl-copy).
    pub fn copy_to_clipboard(&self, ctx: &egui::Context, text: &str) {
        ctx.copy_text(text.to_string());
        let _ = std::process::Command::new("wl-copy").arg(text).spawn();
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
        let tr = self.tr();
        if self.themes.follows_omarchy() && self.selected_theme.is_none() {
            match self.themes.system_theme() {
                Some(theme) => {
                    tr.theme_omarchy_named(fastframe_theme::display_name(&theme.filename))
                }
                None => tr.theme_omarchy.into(),
            }
        } else {
            match &self.selected_theme {
                Some(name) => tr.theme_named(name),
                None => tr.theme_default.into(),
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

    /// El canal por el que el hilo del socket le cuenta a la app lo que le
    /// mandan otras instancias. Se lo pasa `main` al empezar a escuchar.
    pub fn aviso_sender(&self) -> Sender<UpdateMessage> {
        self.update_tx.clone()
    }

    /// Pregunta por las herramientas en un hilo: arrancar la ventana no puede
    /// depender de lanzar procesos. Son dos porque ffmpeg hace falta para unir
    /// pistas y extraer audio, y conviene saberlo antes de bajar 200 MB.
    pub fn check_herramientas(&self) {
        let revisores: [(bool, Revisor); 2] = [
            (true, crate::backend::ytdlp::version),
            (false, crate::backend::ytdlp::version_ffmpeg),
        ];
        for (es_ytdlp, revisar) in revisores {
            let tx = self.update_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(UpdateMessage::Version(es_ytdlp, revisar()));
            });
        }
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
    use super::{App, QueueFilter};

    /// Una lista chica se encola de un toque; una grande pide confirmar. El
    /// limite es cantidad (>=10) o peso estimado (>=1 GB).
    #[test]
    fn las_listas_grandes_piden_confirmacion() {
        assert!(!App::pide_confirmacion(0, None));
        assert!(!App::pide_confirmacion(1, None));
        assert!(!App::pide_confirmacion(App::LISTA_GRANDE - 1, None));
        assert!(App::pide_confirmacion(App::LISTA_GRANDE, None));
        assert!(App::pide_confirmacion(19, None));
        assert!(App::pide_confirmacion(500, None));

        // Por peso: aunque sean pocos videos, si pesa 1 GB o mas se pide confirmar
        assert!(!App::pide_confirmacion(3, Some(500_000_000)));
        assert!(App::pide_confirmacion(3, Some(1_000_000_000)));
        assert!(App::pide_confirmacion(2, Some(2_500_000_000)));
    }

    #[test]
    fn el_idioma_por_defecto_es_ingles_y_cambia_sin_reiniciar() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);
        assert_eq!(app.settings.language, crate::i18n::Language::En);
        assert_eq!(app.tr().settings, "settings");
        assert_eq!(app.tr().queue, "QUEUE");

        app.select_language(crate::i18n::Language::Es);
        assert_eq!(app.settings.language, crate::i18n::Language::Es);
        assert_eq!(app.tr().settings, "ajustes");
        assert_eq!(app.tr().queue, "COLA");
        assert_eq!(app.tr().empty_title, "la cola está vacía");
    }

    #[test]
    fn quita_y_limpia_trabajos_desde_app() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);
        let id1 = {
            let mut q = app.backend.queue.lock().unwrap();
            let id = q.push_ready(
                "https://ejemplo.test/1".into(),
                crate::backend::Options::default(),
                crate::backend::Media::default(),
            );
            q.get_mut(id).unwrap().state = crate::backend::State::Done {
                path: "/tmp/1.mp4".into(),
            };
            id
        };
        let id2 = {
            let mut q = app.backend.queue.lock().unwrap();
            let id = q.push_ready(
                "https://ejemplo.test/2".into(),
                crate::backend::Options::default(),
                crate::backend::Media::default(),
            );
            q.get_mut(id).unwrap().state = crate::backend::State::Downloading;
            id
        };

        // id2 esta activo, no se puede quitar
        assert!(!app.remove_job(id2));
        // id1 esta listo, se quita
        assert!(app.remove_job(id1));

        // Limpiar terminados
        let _id3 = {
            let mut q = app.backend.queue.lock().unwrap();
            let id = q.push_ready(
                "https://ejemplo.test/3".into(),
                crate::backend::Options::default(),
                crate::backend::Media::default(),
            );
            q.get_mut(id).unwrap().state = crate::backend::State::Cancelled;
            id
        };
        assert_eq!(app.clear_finished_jobs(), 1);
        let q = app.backend.queue.lock().unwrap();
        assert_eq!(q.jobs.len(), 1);
        assert_eq!(q.jobs[0].id, id2);
    }

    #[test]
    fn crea_app_sin_problemas() {
        let waker = fastframe_shell::Waker::default();
        let app = App::new(&waker);
        assert!(!app.settings_open);
    }

    #[test]
    fn dibuja_cola_sin_panico() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);
        let ctx = egui::Context::default();
        app.attach(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui::url_bar::show(&mut app, ui);
            crate::ui::queue::show(&mut app, ui);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn dibuja_cola_con_trabajos_y_media_card() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);
        let ctx = egui::Context::default();
        app.attach(&ctx);

        app.preview = Some(crate::backend::Media {
            title: "Video de prueba".into(),
            uploader: "Canal de prueba".into(),
            duration: Some(185.0),
            host: "youtube".into(),
            thumbnail_url: None,
            playlist_count: Some(19),
            url: "https://ejemplo.test/lista".into(),
            filesize: Some(1024 * 1024 * 450),
        });
        app.preview_url = Some("https://ejemplo.test/lista".into());

        {
            let mut q = app.backend.queue.lock().unwrap();
            let id1 = q.push_ready(
                "https://ejemplo.test/1".into(),
                crate::backend::Options::default(),
                crate::backend::Media {
                    title: "Primer video".into(),
                    ..Default::default()
                },
            );
            q.get_mut(id1).unwrap().state = crate::backend::State::Done {
                path: "/tmp/primer_video.mp4".into(),
            };

            let id2 = q.push_ready(
                "https://ejemplo.test/2".into(),
                crate::backend::Options::default(),
                crate::backend::Media {
                    title: "Segundo video".into(),
                    ..Default::default()
                },
            );
            q.get_mut(id2).unwrap().state = crate::backend::State::Downloading;
            q.get_mut(id2).unwrap().progress = 0.45;
            q.get_mut(id2).unwrap().speed = Some(1024.0 * 500.0);
            q.get_mut(id2).unwrap().eta_secs = Some(30);

            let id3 = q.push_ready(
                "https://ejemplo.test/3".into(),
                crate::backend::Options::default(),
                crate::backend::Media {
                    title: "Tercer video".into(),
                    ..Default::default()
                },
            );
            q.get_mut(id3).unwrap().state = crate::backend::State::Failed {
                reason: "HTTP Error 403: Forbidden".into(),
            };
        }

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui::url_bar::show(&mut app, ui);
            crate::ui::media_card::show(&mut app, ui);
            crate::ui::queue::show(&mut app, ui);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn atajos_de_teclado_en_interfaz() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);
        let ctx = egui::Context::default();
        app.attach(&ctx);

        assert!(!app.settings_open);
        assert!(!app.quit_requested);

        // Ctrl+, abre ajustes
        let mut input = egui::RawInput::default();
        let cmd = egui::Modifiers {
            command: true,
            ctrl: true,
            ..Default::default()
        };
        input.events.push(egui::Event::ModifiersChanged(cmd));
        input.events.push(egui::Event::Key {
            key: egui::Key::Comma,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: cmd,
        });
        let mut out = ctx.run_ui(input, |ui| app.handle_shortcuts(ui));
        out.textures_delta.clear();
        assert!(app.settings_open);

        // Escape cierra ajustes
        let mut input = egui::RawInput::default();
        input
            .events
            .push(egui::Event::ModifiersChanged(Default::default()));
        input.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        });
        let mut out = ctx.run_ui(input, |ui| app.handle_shortcuts(ui));
        out.textures_delta.clear();
        assert!(!app.settings_open);

        // Ctrl+Q pide salir
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::ModifiersChanged(cmd));
        input.events.push(egui::Event::Key {
            key: egui::Key::Q,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: cmd,
        });
        let mut out = ctx.run_ui(input, |ui| app.handle_shortcuts(ui));
        out.textures_delta.clear();
        assert!(app.quit_requested);

        // Ctrl+L enfoca el campo de URL
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::ModifiersChanged(cmd));
        input.events.push(egui::Event::Key {
            key: egui::Key::L,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: cmd,
        });
        let mut out = ctx.run_ui(input, |ui| app.handle_shortcuts(ui));
        out.textures_delta.clear();
        let focused = ctx.memory(|m| m.focused());
        assert_eq!(focused, Some(egui::Id::new("url_input")));
    }

    #[test]
    fn filtro_de_cola_visibilidad() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);
        let ctx = egui::Context::default();
        app.attach(&ctx);

        // Encolar tres tipos de trabajos: activo, terminado y fallado
        let (_id1, _id2, _id3) = {
            let mut q = app.backend.queue.lock().unwrap();
            let id1 = q.push_ready(
                "https://ejemplo.test/1".into(),
                crate::backend::Options::default(),
                crate::backend::Media::default(),
            );
            q.get_mut(id1).unwrap().state = crate::backend::State::Downloading;

            let id2 = q.push_ready(
                "https://ejemplo.test/2".into(),
                crate::backend::Options::default(),
                crate::backend::Media::default(),
            );
            q.get_mut(id2).unwrap().state = crate::backend::State::Done {
                path: "/tmp/ok.mp4".into(),
            };

            let id3 = q.push_ready(
                "https://ejemplo.test/3".into(),
                crate::backend::Options::default(),
                crate::backend::Media::default(),
            );
            q.get_mut(id3).unwrap().state = crate::backend::State::Failed {
                reason: "error".into(),
            };

            (id1, id2, id3)
        };

        // Todas
        app.queue_filter = QueueFilter::All;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui::queue::show(&mut app, ui);
        });
        out.textures_delta.clear();

        // Solo activas
        app.queue_filter = QueueFilter::Active;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui::queue::show(&mut app, ui);
        });
        out.textures_delta.clear();

        // Solo listas
        app.queue_filter = QueueFilter::Done;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui::queue::show(&mut app, ui);
        });
        out.textures_delta.clear();

        // Solo errores
        app.queue_filter = QueueFilter::Failed;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui::queue::show(&mut app, ui);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn interaccion_con_ficha_y_formatos() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);

        // Formatos
        assert_eq!(app.options.format_id, "best");
        app.select_format("1080p");
        assert_eq!(app.options.format_id, "1080p");
        assert_eq!(app.settings.format_id, "1080p");

        app.select_format("mp3");
        assert_eq!(app.options.format_id, "mp3");

        app.select_format("m4a");
        assert_eq!(app.options.format_id, "m4a");
        assert_eq!(app.settings.format_id, "m4a");

        // Subtitulos
        app.toggle_subtitle("es");
        assert!(app.settings.subtitles.contains(&"es".to_string()));
        app.toggle_subtitle("en");
        assert!(app.settings.subtitles.contains(&"en".to_string()));
        app.toggle_subtitle("es");
        assert!(!app.settings.subtitles.contains(&"es".to_string()));
    }

    #[test]
    fn dos_toques_en_lista_grande() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);

        app.preview = Some(crate::backend::Media {
            title: "Lista de 20".into(),
            playlist_count: Some(20),
            ..Default::default()
        });
        app.preview_url = Some("https://ejemplo.test/lista20".into());

        assert!(App::pide_confirmacion(20, None));
        assert!(app.confirmar_lista.is_none());

        // Simular primer toque
        let pide = App::pide_confirmacion(20, None);
        let esperando = app.confirmar_lista == Some(20);
        assert!(pide && !esperando);
        app.confirmar_lista = Some(20);

        // Segundo toque
        let esperando2 = app.confirmar_lista == Some(20);
        assert!(esperando2);
        app.confirmar_lista = None;
        app.enqueue_preview();

        let q = app.backend.queue.lock().unwrap();
        assert_eq!(q.jobs.len(), 1);
        assert_eq!(q.jobs[0].url, "https://ejemplo.test/lista20");
    }

    #[test]
    fn pegar_multiples_enlaces_encola_en_lote() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);

        app.url = "https://ejemplo.test/video1\nhttps://ejemplo.test/video2\n  https://ejemplo.test/video3  \n\n".into();
        app.preview_current_url();

        // Debe haber limpiado url y no dejar preview pendiente
        assert_eq!(app.url, "");
        assert!(app.preview.is_none());
        assert!(!app.probing);

        // Debe haber encolado los 3 enlaces directamente
        let q = app.backend.queue.lock().unwrap();
        assert_eq!(q.jobs.len(), 3);
        assert_eq!(q.jobs[0].url, "https://ejemplo.test/video1");
        assert_eq!(q.jobs[1].url, "https://ejemplo.test/video2");
        assert_eq!(q.jobs[2].url, "https://ejemplo.test/video3");
    }

    #[test]
    fn retry_failed_no_reintenta_terminadas() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);

        let (id_done, id_failed, id_cancelled) = {
            let mut q = app.backend.queue.lock().unwrap();
            let j1 = q.push_ready(
                "https://ejemplo.test/done".into(),
                crate::backend::Options::default(),
                crate::backend::Media::default(),
            );
            q.get_mut(j1).unwrap().state = crate::backend::State::Done {
                path: "/tmp/fake.mp4".into(),
            };

            let j2 = q.push_ready(
                "https://ejemplo.test/fail".into(),
                crate::backend::Options::default(),
                crate::backend::Media::default(),
            );
            q.get_mut(j2).unwrap().state = crate::backend::State::Failed {
                reason: "error de red".into(),
            };

            let j3 = q.push_ready(
                "https://ejemplo.test/cancel".into(),
                crate::backend::Options::default(),
                crate::backend::Media::default(),
            );
            q.get_mut(j3).unwrap().state = crate::backend::State::Cancelled;

            (j1, j2, j3)
        };

        // Reintentar fallidas solo debe tomar las 2 con error o canceladas
        let reintentadas = app.retry_failed();
        assert_eq!(reintentadas, 2);

        let q = app.backend.queue.lock().unwrap();
        assert!(matches!(
            q.jobs.iter().find(|j| j.id == id_done).unwrap().state,
            crate::backend::State::Done { .. }
        ));
        assert_eq!(
            q.jobs.iter().find(|j| j.id == id_failed).unwrap().state,
            crate::backend::State::Queued
        );
        assert_eq!(
            q.jobs.iter().find(|j| j.id == id_cancelled).unwrap().state,
            crate::backend::State::Queued
        );
    }

    #[test]
    fn limite_de_velocidad_en_ajustes() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);

        assert_eq!(app.options.rate_limit, None);

        app.settings.rate_limit = "5M".into();
        app.settings_changed();

        assert_eq!(app.options.rate_limit, Some("5M".into()));

        app.settings.rate_limit = "   ".into();
        app.settings_changed();

        assert_eq!(app.options.rate_limit, None);
    }

    #[test]
    fn busqueda_en_cola_y_limpieza_con_escape() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);
        let ctx = egui::Context::default();

        app.queue_search = "tutorial".into();
        assert_eq!(app.queue_search, "tutorial");

        // Al presionar Escape, debe limpiar la búsqueda
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });

        let mut out = ctx.run_ui(input, |ui| {
            app.handle_shortcuts(ui);
        });
        out.textures_delta.clear();

        assert_eq!(app.queue_search, "");
    }

    #[test]
    fn atajo_ctrl_f_enfoca_buscador() {
        let waker = fastframe_shell::Waker::default();
        let mut app = App::new(&waker);
        let ctx = egui::Context::default();
        let cmd = egui::Modifiers::COMMAND;

        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::ModifiersChanged(cmd));
        input.events.push(egui::Event::Key {
            key: egui::Key::F,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: cmd,
        });

        let mut out = ctx.run_ui(input, |ui| {
            app.handle_shortcuts(ui);
        });
        out.textures_delta.clear();

        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new("queue_search_input"))
        );
    }
}
