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
mod icon;
mod palette;
mod settings;
mod ui;
mod updates;

use app::{App, APP_NAME};

struct Window {
    app: fastframe_shell::Held<App>,
    recovery_checked: bool,
}

impl eframe::App for Window {
    // `logic` corre antes de dibujar y recibe el Context: es donde van las
    // cosas de ventana, no de interfaz.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !std::mem::replace(&mut self.recovery_checked, true) {
            fastframe_shell::window::recover_offscreen(ctx, frame);
        }
        self.app.tick(ctx);
    }

    // En egui 0.36 la app recibe un `Ui` raiz y los paneles se muestran
    // dentro de el, en vez de colgar del Context.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
}

fn main() -> anyhow::Result<()> {
    // `--settings` es de la app, no del actualizador: se saca de los
    // argumentos antes de pasearlos por ahi. Abre el panel al arrancar, que es
    // lo que quiere un acceso directo del menu.
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    let open_settings = take_flag(&mut arguments, "--settings");

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
    if open_settings {
        app.open_settings();
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
                    Ok(Box::new(Window {
                        app,
                        recovery_checked: false,
                    }))
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

fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_app_id("reel")
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([760.0, 520.0]),
        ..Default::default()
    }
}
