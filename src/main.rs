//! reel: pegas un enlace, elegis un formato, y la cola hace el resto.
//!
//! La ventana es egui sobre eframe. Todo lo que no es la interfaz (tipografia,
//! tema, iconos, log, tray, vida sin ventana, actualizaciones) viene de
//! fastframe, que es justo el reparto que propone su guia: fastframe se queda
//! con lo que es igual en toda app, y la app se queda con su interfaz.

mod app;
mod backend;
mod dirs;
mod fonts;
mod i18n;
mod icon;
mod instancia;
mod palette;
mod settings;
mod ui;
mod updates;

use app::{App, APP_NAME};

struct Window {
    app: fastframe_shell::Held<App>,
}

impl eframe::App for Window {
    // `logic` corre antes de dibujar y recibe el Context: es donde van las
    // cosas de ventana, no de interfaz.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.tick(ctx);
    }

    // En egui 0.36 la app recibe un `Ui` raiz y los paneles se muestran
    // dentro de el, en vez de colgar del Context.
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Se comprueba cada vez y no una sola vez: fastframe-shell vuelve a
        // crear la ventana cada vez que se muestra desde el tray, asi que un
        // "ya lo revise" dejaba sin revisar justo las ventanas siguientes.
        // `recover_offscreen` no hace nada en Wayland y es barato en el resto.
        fastframe_shell::window::recover_offscreen(ui.ctx(), frame);
        self.app.ui(ui);
    }
}

fn main() -> anyhow::Result<()> {
    // Las pruebas de la cola corren en un proceso propio: tocan `REEL_YTDLP` y
    // no quieren abrir ventana ni ensuciar el log. Van detras de una feature
    // para que el binario normal no cargue con ellas.
    #[cfg(feature = "selfcheck")]
    if let Some(modo) = selfcheck_mode() {
        std::process::exit(backend::selfcheck::run(&modo));
    }

    // `--settings` es de la app, no del actualizador: se saca de los
    // argumentos antes de pasearlos por ahi. Abre el panel al arrancar, que es
    // lo que quiere un acceso directo del menu.
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    let open_settings = take_flag(&mut arguments, "--settings");
    // `--yoink URL`: lee el enlace y lo encola sin abrir nada mas. Sirve para
    // un atajo del escritorio o para mandarle algo desde un script.
    let yoink = take_flag_value(&mut arguments, "--yoink");

    // Si ya hay una instancia corriendo, se le pasa lo que se pidio y esta
    // copia se va sin abrir nada: dos ventanas pelearian por el mismo icono de
    // bandeja y por el mismo archivo de estado.
    let escucha = match instancia::tomar() {
        Some(listener) => Some(listener),
        None => {
            let aviso = match &yoink {
                Some(url) => instancia::Aviso::Yoink(url.clone()),
                None => instancia::Aviso::Mostrar,
            };
            if instancia::avisar(&aviso) {
                println!("reel ya estaba abierto: le pase el pedido y me voy");
                return Ok(());
            }
            // El socket estaba pero nadie contesta: se sigue como si nada.
            None
        }
    };

    // Primero el ayudante de actualizacion, que puede quedarse con los
    // argumentos y terminar sin abrir ventana.
    let launch = fastframe_update::intercept(&updates::UPDATES);
    let start_hidden = launch
        .arguments
        .iter()
        .any(|argument| argument == "--start-hidden");

    // El log necesita su carpeta antes de que fastframe-log abra el archivo.
    dirs::ensure_state_dir();

    fastframe_log::Logging::new("reel", env!("CARGO_PKG_VERSION"))
        .filter("warn,reel=info")
        .file(dirs::log_file())
        .panic_log(dirs::panic_log())
        .init()?;

    if let Some(error) = launch.error {
        log::warn!("la actualizacion anterior se revertio: {error}");
    }

    let waker = fastframe_shell::Waker::default();
    let mut app = App::new(&waker);
    app.start_themes();
    app.check_updates();
    app.check_herramientas();
    if open_settings {
        app.open_settings();
    }
    if let Some(url) = yoink {
        app.yoink(url);
    }
    if let Some(listener) = escucha {
        let avisos = app.aviso_sender();
        instancia::escuchar(listener, move |aviso| {
            let _ = avisos.send(app::UpdateMessage::Aviso(aviso));
        });
    }

    if let Some(receipt) = launch.receipt {
        std::thread::spawn(move || receipt.acknowledge());
    }

    fastframe_shell::Shell::new(app, &waker)
        .start_hidden(start_hidden)
        .idle(fastframe_tray::idle)
        .run(|lease| {
            eframe::run_native(
                APP_NAME,
                native_options(),
                Box::new(move |cc| {
                    let mut app = lease.take(&cc.egui_ctx);
                    app.attach(&cc.egui_ctx);
                    Ok(Box::new(Window { app }))
                }),
            )
        })
        // eframe::Error no es Send + Sync, asi que anyhow no lo acepta con `?`.
        .map_err(|error| anyhow::anyhow!("no pude abrir la ventana: {error}"))?;

    Ok(())
}

/// Saca una bandera de los argumentos y dice si estaba. Asi una bandera
/// nuestra no le llega al actualizador como si fuera basura.
fn take_flag(arguments: &mut Vec<String>, flag: &str) -> bool {
    let before = arguments.len();
    arguments.retain(|argument| argument != flag);
    arguments.len() != before
}

/// Saca una bandera con su valor, como `--yoink URL`. Devuelve `None` cuando
/// la bandera no esta o no trae nada detras.
fn take_flag_value(arguments: &mut Vec<String>, flag: &str) -> Option<String> {
    let at = arguments.iter().position(|argument| argument == flag)?;
    // Se saca la bandera y, si hay, el valor que la sigue.
    arguments.remove(at);
    if at < arguments.len() {
        Some(arguments.remove(at)).filter(|value| !value.trim().is_empty())
    } else {
        None
    }
}

/// `--download-selfcheck MODO`, para las pruebas de la cola.
#[cfg(feature = "selfcheck")]
fn selfcheck_mode() -> Option<String> {
    let mut argumentos = std::env::args().skip(1);
    while let Some(argumento) = argumentos.next() {
        if argumento == "--download-selfcheck" {
            return argumentos.next();
        }
    }
    None
}

/// Como se abre la ventana.
///
/// El tamano de aca es el del primer arranque: con la feature `persistence`,
/// eframe guarda el que dejo el usuario y lo repone. El icono es el mismo que
/// el del tray, dibujado en `icon.rs`, y `app_id` es ademas el nombre con el
/// que el escritorio reconoce la ventana (lo que Hyprland matchea en sus
/// reglas) y el directorio donde eframe guarda ese estado.
fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_app_id("reel")
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([760.0, 520.0])
            .with_icon(window_icon()),
        // Solo decide la posicion del primer arranque: despues manda lo que
        // haya guardado la persistencia.
        centered: true,
        ..Default::default()
    }
}

/// El icono de la ventana, del mismo dibujo que el del tray. El lado tiene que
/// ser multiplo de 4, como pide `IconData`.
fn window_icon() -> egui::IconData {
    const LADO: usize = 256;
    egui::IconData {
        rgba: crate::icon::app_icon_rgba(LADO),
        width: LADO as u32,
        height: LADO as u32,
    }
}
