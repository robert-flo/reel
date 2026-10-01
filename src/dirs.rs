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

pub fn log_file() -> PathBuf {
    state_dir().join("reel.log")
}

pub fn panic_log() -> PathBuf {
    state_dir().join("panic.log")
}
