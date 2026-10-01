//! Los ajustes que el usuario cambia y que sobreviven al cierre.
//!
//! Es lo que el README promete y que hasta ahora no tenia donde vivir: carpeta
//! de salida, plantilla del nombre del archivo, cookies del navegador y el
//! tema elegido. `Options` sigue siendo lo que se le pasa a yt-dlp por
//! trabajo; esto es lo que dura entre arranques.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::dirs;
use crate::i18n::Language;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Vacio significa la carpeta del sistema: Videos o Music segun el
    /// formato, que es como se comporta yt-dlp cuando no se le dice nada.
    pub output_dir: String,
    /// Plantilla de `-o`. Vacia significa el default de la app.
    pub filename_template: String,
    /// El id del formato elegido ("best", "1080p", "mp3"...). Vacio es "best".
    pub format_id: String,
    /// Nombre del navegador para `--cookies-from-browser`. Vacio es apagado.
    pub cookies_browser: String,
    /// Idiomas de subtitulos. Vacio es apagado.
    #[serde(default)]
    pub subtitles: Vec<String>,
    /// Limite de velocidad para `--limit-rate`. Vacio es sin limite (ej: "5M", "1M").
    #[serde(default)]
    pub rate_limit: String,
    /// Archivo de paleta elegido. `None` es seguir el tema del escritorio.
    pub theme: Option<String>,
    /// Idioma de la interfaz. Vacio o desconocido es ingles.
    #[serde(default)]
    pub language: Language,
}

impl Settings {
    /// Lee `settings.json`. Un archivo que falta es un estreno, no un error;
    /// uno ilegible se avisa y se arranca con los valores por defecto, porque
    /// quedarse sin abrir por un JSON roto seria peor.
    pub fn load() -> Self {
        let path = dirs::settings_file();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("no pude leer {}: {error}", path.display());
                }
                return Self::default();
            }
        };

        match serde_json::from_str(&text) {
            Ok(settings) => settings,
            Err(error) => {
                log::warn!(
                    "{} no se entiende, uso los ajustes por defecto: {error}",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Escribe `settings.json` entero. Se guarda tras cada cambio, asi que no
    /// hay nada que confirmar ni que se pierda si la app se cierra de golpe.
    pub fn save(&self) -> std::io::Result<()> {
        let path = dirs::settings_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        std::fs::write(&path, text)
    }

    /// La plantilla que de verdad se usa: la elegida o la de la app.
    pub fn template(&self) -> &str {
        let chosen = self.filename_template.trim();
        if chosen.is_empty() {
            crate::backend::DEFAULT_TEMPLATE
        } else {
            chosen
        }
    }

    /// La carpeta de salida elegida, si es que hay una.
    pub fn output_path(&self) -> Option<PathBuf> {
        let trimmed = self.output_dir.trim();
        (!trimmed.is_empty()).then(|| PathBuf::from(expand_home(trimmed)))
    }

    /// Los idiomas de subtitulos como los espera `--sub-langs`, que es una
    /// lista separada por comas. `None` cuando no se pidio ninguno.
    pub fn subtitle_languages(&self) -> Option<String> {
        let limpios: Vec<&str> = self
            .subtitles
            .iter()
            .map(|idioma| idioma.trim())
            .filter(|idioma| !idioma.is_empty())
            .collect();
        (!limpios.is_empty()).then(|| limpios.join(","))
    }

    /// El formato elegido, ya comprobado contra los que existen. Un id que ya
    /// no esta —porque se saco un preset o el archivo se edito a mano— vuelve
    /// al primero en vez de dejar la cola sin formato.
    pub fn format(&self) -> &'static crate::backend::Format {
        let guardado = crate::backend::FORMATS
            .iter()
            .any(|format| format.id == self.format_id);
        if guardado {
            crate::backend::format_by_id(&self.format_id)
        } else {
            &crate::backend::FORMATS[0]
        }
    }

    /// Los idiomas como se escriben en el panel: `es, en`.
    pub fn subtitle_list(&self) -> String {
        self.subtitles.join(", ")
    }

    /// Guarda idiomas escritos a mano, como `es, en`. Los codigos se limpian de
    /// espacios y se descartan los repetidos: pedir dos veces lo mismo no
    /// cambia nada y solo ensucia el archivo.
    pub fn set_subtitles(&mut self, texto: &str) {
        let mut limpios: Vec<String> = Vec::new();
        for idioma in texto.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if !limpios.iter().any(|ya| ya == idioma) {
                limpios.push(idioma.to_string());
            }
        }
        self.subtitles = limpios;
    }
}

/// `~/Videos` como lo escribe una persona, para no obligar a nadie a tipear
/// `/home/quien`. `$HOME` tambien, que es lo que se pega de una terminal.
fn expand_home(path: &str) -> String {
    for prefix in ["~/", "$HOME/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            if let Some(home) = std::env::var_os("HOME") {
                return Path::new(&home).join(rest).display().to_string();
            }
        }
    }
    path.to_string()
}

/// Lo que se puede verificar de una carpeta sin intentar escribir en ella:
/// que exista, que sea una carpeta, y que no sea de solo lectura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirProblem {
    Missing,
    NotADirectory,
    NotWritable,
}

/// `None` cuando no hay nada que objetar. Una ruta todavia a medio tipear se
/// reporta como inexistente, que es cierto y no rompe nada.
pub fn check_dir(path: &Path) -> Option<DirProblem> {
    if !path.exists() {
        return Some(DirProblem::Missing);
    }
    if !path.is_dir() {
        return Some(DirProblem::NotADirectory);
    }
    if std::fs::metadata(path).is_ok_and(|meta| meta.permissions().readonly()) {
        return Some(DirProblem::NotWritable);
    }
    None
}

/// Los navegadores que yt-dlp sabe leer y que parecen estar en esta maquina.
/// Es solo para poblar la lista: igual se puede escribir otro a mano.
pub fn browsers() -> Vec<String> {
    KNOWN
        .iter()
        .filter(|(_, dirs)| dirs.iter().any(|dir| expanded_dir(dir).is_some()))
        .map(|(name, _)| (*name).to_owned())
        .collect()
}

/// El navegador que el escritorio usa por defecto, para no hardcodear uno.
/// Cae al primero que se haya detectado, y de ultimo a `firefox`.
pub fn detect_browser() -> String {
    if let Some(name) = default_browser_from_desktop() {
        return name;
    }
    browsers()
        .into_iter()
        .next()
        .unwrap_or_else(|| "firefox".into())
}

/// El nombre del navegador tal como lo devuelve el escritorio, ya limpio.
fn default_browser_from_desktop() -> Option<String> {
    let output = std::process::Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let desktop = String::from_utf8_lossy(&output.stdout);
    let trimmed = desktop.trim();
    let stem = trimmed
        .strip_suffix(".desktop")
        .unwrap_or(trimmed)
        .to_lowercase();
    if stem.is_empty() {
        return None;
    }

    Some(normalize_browser(&stem))
}

/// El nombre de un `.desktop` de navegador al nombre que espera yt-dlp:
/// `google-chrome` es `chrome`, `zen-browser` es `zen`, y asi. Lo que no
/// reconocemos se deja tal cual y yt-dlp dira si no le sirve.
fn normalize_browser(stem: &str) -> String {
    const KNOWN_NAMES: &[(&str, &str)] = &[
        ("firefox", "firefox"),
        ("chromium", "chromium"),
        ("google-chrome", "chrome"),
        ("chrome", "chrome"),
        ("brave", "brave"),
        ("vivaldi", "vivaldi"),
        ("opera", "opera"),
        ("microsoft-edge", "edge"),
        ("edge", "edge"),
        ("zen", "zen"),
        ("librewolf", "librewolf"),
    ];

    KNOWN_NAMES
        .iter()
        .find(|(needle, _)| stem.contains(needle))
        .map(|(_, name)| (*name).to_owned())
        .unwrap_or_else(|| stem.to_owned())
}

/// Navegadores que yt-dlp lee, con las carpetas que delatan su presencia.
const KNOWN: &[(&str, &[&str])] = &[
    ("firefox", &["~/.mozilla/firefox"]),
    ("chromium", &["~/.config/chromium"]),
    ("chrome", &["~/.config/google-chrome"]),
    ("brave", &["~/.config/BraveSoftware"]),
    ("vivaldi", &["~/.config/vivaldi"]),
    ("opera", &["~/.config/opera"]),
    ("edge", &["~/.config/microsoft-edge"]),
    ("zen", &["~/.config/zen"]),
    ("librewolf", &["~/.librewolf"]),
];

/// Una carpeta de la lista, ya con `~` resuelto, si existe.
fn expanded_dir(path: &str) -> Option<PathBuf> {
    let expanded = expand_home(path);
    Path::new(&expanded)
        .is_dir()
        .then(|| PathBuf::from(expanded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expande_el_home() {
        // `$HOME` siempre esta puesto en el entorno de las pruebas.
        let home = std::env::var("HOME").expect("HOME deberia estar");
        assert_eq!(expand_home("~/Videos"), format!("{home}/Videos"));
        assert_eq!(expand_home("$HOME/Music"), format!("{home}/Music"));
        assert_eq!(expand_home("/tmp/reel"), "/tmp/reel");
    }

    #[test]
    fn la_plantilla_vacia_es_la_de_la_app() {
        let settings = Settings::default();
        assert_eq!(settings.template(), crate::backend::DEFAULT_TEMPLATE);
        assert_eq!(settings.output_path(), None);

        let settings = Settings {
            filename_template: "  %(id)s.%(ext)s  ".into(),
            output_dir: "  ~/Videos  ".into(),
            ..Settings::default()
        };
        assert_eq!(settings.template(), "%(id)s.%(ext)s");
        assert!(settings.output_path().is_some());
    }

    #[test]
    fn los_subtitulos_se_limpian_y_no_se_reptien() {
        let mut settings = Settings::default();
        assert_eq!(settings.subtitle_languages(), None);
        assert_eq!(settings.subtitle_list(), "");

        settings.set_subtitles(" es , en ,, es , fr ");
        assert_eq!(settings.subtitles, vec!["es", "en", "fr"]);
        assert_eq!(settings.subtitle_languages(), Some("es,en,fr".into()));
        assert_eq!(settings.subtitle_list(), "es, en, fr");

        // Vaciar es apagar los subtitulos.
        settings.set_subtitles("   ");
        assert_eq!(settings.subtitle_languages(), None);
    }

    /// Un formato guardado que ya no existe no puede dejar la cola sin
    /// formato: se vuelve al primero.
    #[test]
    fn un_formato_que_ya_no_existe_vuelve_al_primero() {
        let mut settings = Settings::default();
        assert_eq!(settings.format().id, "best");

        settings.format_id = "1080p".into();
        assert_eq!(settings.format().id, "1080p");

        settings.format_id = "formato-que-no-existe".into();
        assert_eq!(settings.format().id, "best");

        settings.format_id.clear();
        assert_eq!(settings.format().id, "best");
    }

    #[test]
    fn reconoce_el_nombre_del_navegador() {
        assert_eq!(normalize_browser("google-chrome"), "chrome");
        assert_eq!(normalize_browser("zen-browser"), "zen");
        assert_eq!(normalize_browser("firefox"), "firefox");
        assert_eq!(normalize_browser("navegador-raro"), "navegador-raro");
    }

    #[test]
    fn una_carpeta_que_no_existe_se_avisa() {
        assert_eq!(
            check_dir(Path::new("/no/existe/reel-de-prueba")),
            Some(DirProblem::Missing)
        );
        assert_eq!(check_dir(Path::new("/tmp")), None);
    }

    #[test]
    fn el_idioma_falta_es_ingles_y_se_guarda() {
        let settings = Settings::default();
        assert_eq!(settings.language, Language::En);

        let settings = Settings {
            language: Language::Es,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).expect("serde");
        assert!(json.contains("\"language\": \"es\"") || json.contains("\"language\":\"es\""));

        let loaded: Settings = serde_json::from_str(&json).expect("roundtrip");
        assert_eq!(loaded.language, Language::Es);

        let sin_campo: Settings = serde_json::from_str("{}").expect("default");
        assert_eq!(sin_campo.language, Language::En);
    }
}
