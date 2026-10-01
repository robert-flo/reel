//! Donde viven la configuracion, los temas y el log.

use std::path::PathBuf;

use crate::app::SLUG;

fn project() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("dev", "", SLUG)
}

pub fn config_dir() -> PathBuf {
    project()
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join(SLUG))
}

pub fn state_dir() -> PathBuf {
    project()
        .and_then(|p| p.state_dir().map(|d| d.to_path_buf()))
        .unwrap_or_else(config_dir)
}

/// Deja listo el directorio de estado. Hace falta antes de inicializar el log:
/// fastframe-log abre el archivo pero no crea la carpeta, asi que en una
/// instalacion nueva el log se perdia entero y en silencio. Un fallo aca no
/// impide arrancar, solo se queda sin log.
pub fn ensure_state_dir() {
    if let Err(error) = std::fs::create_dir_all(state_dir()) {
        eprintln!("no pude crear {}: {error}", state_dir().display());
    }
}

/// Los ajustes del usuario, en config y no en state: son suyos, se editan a
/// mano si hace falta y no se pierden al limpiar el estado.
pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn log_file() -> PathBuf {
    state_dir().join("reel.log")
}

pub fn panic_log() -> PathBuf {
    state_dir().join("panic.log")
}
