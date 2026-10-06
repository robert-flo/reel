//! Textos de la interfaz, en el idioma que eligio el usuario.
//!
//! El ingles es la fuente y el respaldo: un ajuste que falta o un codigo que
//! no reconocemos arranca en ingles. El espanol latinoamericano neutro es el
//! primer idioma extra, como prueba de que el cableado anda.
//!
//! Para agregar un idioma:
//! 1. Suma una variante a `Language` y su codigo en `parse` / `code` / `ALL`.
//! 2. Agrega el campo `xx: "..."` en cada entrada de `i18n_strings!`.
//! 3. Expone el catalogo en `Language::catalog`.
//!
//! El compilador obliga a completar las cadenas; no hay huecos silenciosos.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    En,
    Es,
}

impl Language {
    pub const ALL: &[Language] = &[Language::En, Language::Es];

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "es" | "es-419" | "es-mx" | "es-ar" | "es-es" | "spanish" | "espanol" | "español" => {
                Self::Es
            }
            _ => Self::En,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Es => "es",
        }
    }

    /// El nombre en su propio idioma, para el selector.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Es => "Español",
        }
    }

    pub fn catalog(self) -> &'static Catalog {
        match self {
            Self::En => &EN,
            Self::Es => &ES,
        }
    }
}

impl Serialize for Language {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(Self::parse(&value))
    }
}

macro_rules! i18n_strings {
    ($($field:ident: { en: $en:expr, es: $es:expr $(,)? })*) => {
        #[derive(Clone, Copy, Debug)]
        pub struct Catalog {
            $(pub $field: &'static str,)*
        }

        pub static EN: Catalog = Catalog {
            $($field: $en,)*
        };

        pub static ES: Catalog = Catalog {
            $($field: $es,)*
        };
    };
}

i18n_strings! {
    settings: { en: "settings", es: "ajustes" }
    url_hint: { en: "paste a link", es: "pega un enlace" }
    paste: { en: "paste", es: "pegar" }
    clear: { en: "clear", es: "limpiar" }
    search: { en: "search", es: "buscar" }
    reading: { en: "reading", es: "leyendo" }

    reading_link: { en: "reading the link...", es: "leyendo el enlace..." }
    could_not_read: { en: "couldn't read that link", es: "no pude leer ese enlace" }
    no_media_found: { en: "no media found in this link", es: "no se encontraron archivos multimedia en el enlace" }
    copy_error: { en: "copy error", es: "copiar error" }
    dismiss: { en: "dismiss", es: "descartar" }
    untitled: { en: "Untitled", es: "Sin título" }
    playlist_prefix: { en: "playlist:", es: "es una lista:" }
    video_one: { en: "video", es: "video" }
    video_many: { en: "videos", es: "videos" }
    confirm: { en: "confirm", es: "confirmar" }
    enqueue: { en: "queue", es: "encolar" }
    download: { en: "download", es: "descargar" }
    chapters: { en: "chapters", es: "capítulos" }
    sponsorblock: { en: "sponsorblock", es: "sponsorblock" }
    sponsorblock_tip: { en: "skip sponsor segments with SponsorBlock", es: "salta patrocinios con SponsorBlock" }
    metadata_artwork: { en: "metadata + artwork", es: "metadatos + carátula" }
    subtitles: { en: "subtitles", es: "subtítulos" }
    browser_cookies: { en: "browser cookies", es: "cookies del navegador" }
    clip: { en: "clip", es: "recortar" }
    clip_tip: { en: "download only a specific time segment", es: "descarga únicamente un fragmento de tiempo específico" }
    clip_from: { en: "from:", es: "desde:" }
    clip_to: { en: "to:", es: "hasta:" }
    format_best: { en: "Best", es: "Mejor" }
    format_video: { en: "video", es: "video" }
    format_audio_only: { en: "audio only", es: "solo audio" }

    queue: { en: "QUEUE", es: "COLA" }
    filter_all: { en: "all", es: "todas" }
    filter_active: { en: "in queue", es: "en cola" }
    filter_done: { en: "done", es: "terminadas" }
    filter_skipped: { en: "skipped", es: "omitidas" }
    filter_failed: { en: "failed", es: "con error" }
    search_hint: { en: "search...", es: "buscar..." }
    retry_failed: { en: "retry failed", es: "reintentar fallidas" }
    clear_finished: { en: "clear finished", es: "limpiar terminadas" }
    cancel_active: { en: "cancel active", es: "cancelar activas" }
    active_one: { en: "in queue", es: "en cola" }
    active_many: { en: "in queue", es: "en cola" }
    completed_one: { en: "completed", es: "completada" }
    completed_many: { en: "completed", es: "completadas" }
    skipped_one: { en: "skipped", es: "omitida" }
    skipped_many: { en: "skipped", es: "omitidas" }
    failed_one: { en: "failed", es: "fallida" }
    failed_many: { en: "failed", es: "fallidas" }
    no_match_prefix: { en: "no downloads match", es: "ninguna descarga coincide con" }
    no_active: { en: "no active downloads", es: "no hay descargas activas" }
    no_done: { en: "no finished downloads", es: "no hay descargas terminadas" }
    no_skipped: { en: "no skipped downloads", es: "no hay descargas omitidas" }
    no_failed: { en: "no failed downloads", es: "no hay descargas con error" }
    clear_search: { en: "clear search", es: "limpiar búsqueda" }
    empty_title: { en: "the queue is empty", es: "la cola está vacía" }
    empty_hint: { en: "paste a link above and it shows up here", es: "pega un enlace arriba y aparece aquí" }
    now_downloading: { en: "DOWNLOADING NOW", es: "DESCARGANDO AHORA" }
    now_processing: { en: "PROCESSING", es: "PROCESANDO" }
    now_retrying: { en: "RETRYING", es: "REINTENTANDO" }
    waiting_turn: { en: "waiting turn...", es: "esperando turno..." }
    memory_label: { en: "RAM", es: "RAM" }
    memory_tooltip: { en: "Resident memory (RSS) used by reel", es: "Memoria residente (RSS) usada por reel" }
    status_probing: { en: "reading the link", es: "leyendo el enlace" }
    status_queued: { en: "queued", es: "en espera" }
    status_skipped: { en: "skipped", es: "omitido" }
    already_on_disk: { en: "already on disk", es: "ya existía en el disco" }
    status_downloading: { en: "downloading", es: "descargando" }
    status_waiting_ffmpeg: { en: "waiting for ffmpeg", es: "esperando ffmpeg" }
    status_done: { en: "done", es: "listo" }
    status_failed: { en: "failed", es: "falló" }
    status_cancelled: { en: "cancelled", es: "cancelado" }
    status_retrying: { en: "retrying", es: "reintentando" }
    remaining: { en: "left", es: "restante" }
    cancel: { en: "cancel", es: "cancelar" }
    retry: { en: "retry", es: "reintentar" }
    redownload: { en: "download again", es: "volver a bajar" }
    copy_path: { en: "copy path", es: "copiar ruta" }
    copy_url: { en: "copy link", es: "copiar enlace" }
    copy_all_links: { en: "copy links", es: "copiar enlaces" }
    copy_all_links_tip: { en: "copy all queue URLs to clipboard (separated by lines)", es: "copia todas las URLs de la cola al portapapeles (separadas por línea)" }
    folder: { en: "folder", es: "carpeta" }
    open: { en: "open", es: "abrir" }
    open_folder: { en: "open folder", es: "abrir carpeta" }
    remove: { en: "remove", es: "quitar" }
    pp_merger: { en: "merging tracks", es: "uniendo pistas" }
    pp_extract: { en: "extracting audio", es: "extrayendo el audio" }
    pp_thumb: { en: "embedding artwork", es: "incrustando la carátula" }
    pp_generic: { en: "postprocessing", es: "posprocesando" }
    playlist_below: { en: "below", es: "abajo" }

    section_language: { en: "LANGUAGE", es: "IDIOMA" }
    section_output: { en: "OUTPUT FOLDER", es: "CARPETA DE SALIDA" }
    section_format: { en: "FORMAT", es: "FORMATO" }
    section_filename: { en: "FILE NAME", es: "NOMBRE DEL ARCHIVO" }
    section_rate: { en: "SPEED LIMIT", es: "LÍMITE DE VELOCIDAD" }
    section_extras: { en: "EXTRAS", es: "EXTRAS" }
    sponsorblock_toggle: { en: "remove sponsor segments", es: "quitar patrocinios" }
    sponsorblock_hint: { en: "automatically cut out sponsored segments in YouTube videos", es: "corta automáticamente los segmentos patrocinados en videos de YouTube" }
    chapters_toggle: { en: "embed chapters", es: "incrustar capítulos" }
    chapters_hint: { en: "adds chapter markers to video files when available", es: "agrega marcas de capítulos en el video si están disponibles" }
    metadata_toggle: { en: "embed metadata & cover", es: "incrustar metadatos y carátula" }
    metadata_hint: { en: "adds tags, description, and cover art to the file", es: "agrega etiquetas, descripción y carátula al archivo" }
    inhibit_sleep_toggle: { en: "prevent sleep while downloading", es: "evitar suspensión al descargar" }
    inhibit_sleep_hint: { en: "keeps the computer awake so active downloads are not interrupted", es: "mantiene el equipo despierto para no interrumpir descargas activas" }
    section_advanced: { en: "ADVANCED", es: "AVANZADO" }
    extra_args_hint: { en: "additional arguments passed to yt-dlp (e.g. --proxy socks5://...)", es: "argumentos adicionales pasados a yt-dlp (ej.: --proxy socks5://...)" }
    section_subtitles: { en: "SUBTITLES", es: "SUBTÍTULOS" }
    section_cookies: { en: "BROWSER COOKIES", es: "COOKIES DEL NAVEGADOR" }
    section_theme: { en: "THEME", es: "TEMA" }
    output_empty: { en: "empty: ~/Videos for video and ~/Music for audio", es: "vacía: ~/Videos para video y ~/Music para audio" }
    output_saved_prefix: { en: "saved in", es: "se guarda en" }
    output_missing: { en: "that folder doesn't exist yet", es: "esa carpeta todavía no existe" }
    output_not_dir: { en: "that path is a file", es: "esa ruta es un archivo" }
    output_not_writable: { en: "that folder is read-only", es: "esa carpeta es de solo lectura" }
    filename_standard: { en: "standard", es: "estándar" }
    filename_uploader: { en: "with channel/author", es: "con canal/autor" }
    filename_numbered: { en: "numbered", es: "numerado" }
    filename_date: { en: "date and title", es: "fecha y título" }
    filename_hint: { en: "yt-dlp template; empty uses %(title).120s.%(ext)s", es: "plantilla de yt-dlp; vacía usa %(title).120s.%(ext)s" }
    rate_unlimited: { en: "no limit", es: "sin límite" }
    rate_custom_hint: { en: "or type a value (e.g. 500K, 3M)", es: "o escribe un valor (ej.: 500K, 3M)" }
    rate_hint: { en: "caps the bandwidth yt-dlp uses so it doesn't saturate your connection", es: "limita el ancho de banda que usa yt-dlp para no saturar tu conexión" }
    lang_spanish: { en: "Spanish", es: "español" }
    lang_english: { en: "English", es: "inglés" }
    lang_portuguese: { en: "Portuguese", es: "portugués" }
    lang_french: { en: "French", es: "francés" }
    subtitles_embedded: { en: "embedded in the file", es: "se incrustan en el archivo" }
    subtitles_off: { en: "off; they get added to the video", es: "apagados; se agregan al video" }
    use_cookies: { en: "use cookies", es: "usar cookies" }
    no_browsers: { en: "no browsers found here; the name is passed to yt-dlp as-is", es: "no encontré navegadores aquí; el nombre se le pasa tal cual a yt-dlp" }
    browsers_detected_prefix: { en: "detected:", es: "detectados:" }
    browsers_detected_suffix: { en: "for logged-in content", es: "para contenido con sesión" }
    follow_desktop: { en: "follow the desktop", es: "seguir el escritorio" }

    theme_omarchy: { en: "theme: following omarchy", es: "tema: siguiendo omarchy" }
    theme_omarchy_prefix: { en: "theme: following omarchy", es: "tema: siguiendo omarchy" }
    theme_named_prefix: { en: "theme:", es: "tema:" }
    theme_default: { en: "theme: default", es: "tema: por defecto" }
    missing_ytdlp: { en: "without yt-dlp I can't download anything", es: "sin yt-dlp no puedo bajar nada" }
    missing_ffmpeg: { en: "without ffmpeg I can't merge or convert", es: "sin ffmpeg no puedo unir ni convertir" }
    missing_ffmpeg_extra: { en: "without ffmpeg the file is kept as-is: tracks aren't merged, audio isn't extracted, and metadata isn't embedded", es: "sin ffmpeg se baja el archivo tal como viene: no se unen pistas, no se extrae audio y no se incrustan metadatos" }
    update_available_suffix: { en: "available  ·  update", es: "disponible  ·  actualizar" }
    update_downloading: { en: "downloading update", es: "descargando actualización" }
    update_restart: { en: "restart to update", es: "reiniciar para actualizar" }
    update_check_failed: { en: "couldn't check for updates", es: "no pude revisar actualizaciones" }

    notify_download_one: { en: "download", es: "descarga" }
    notify_download_many: { en: "downloads", es: "descargas" }
    notify_failed_word: { en: "failed", es: "falló" }
    notify_done_word: { en: "ready", es: "listo" }
    notify_mixed_ready: { en: "ready", es: "listas" }
    notify_mixed_failed: { en: "with errors", es: "con error" }

    tray_show: { en: "Show reel", es: "Mostrar reel" }
    tray_paste: { en: "Paste and download", es: "Pegar y descargar" }
    tray_quit: { en: "Quit", es: "Salir" }

    tip_forbidden: { en: "The site rejected the request; this often happens if you ask too many times in a row. Try again in a bit.", es: "El sitio rechazó el pedido; suele pasar si pides muchas veces seguidas. Prueba de nuevo en un rato." }
    tip_private: { en: "This looks like it needs a login: try browser cookies.", es: "Parece que hace falta iniciar sesión: prueba con las cookies del navegador." }
    tip_unavailable: { en: "This video is no longer available.", es: "El video ya no está disponible en el sitio." }
    tip_format: { en: "That format isn't available for this video: pick another.", es: "Ese formato no existe para este video: elige otro." }
    tip_timeout: { en: "The connection timed out: retrying usually works.", es: "Se cortó la conexión: reintentar suele alcanzar." }
    tip_no_media: { en: "No multimedia files were found in this link.", es: "No se encontraron archivos multimedia en el enlace." }
}

impl Catalog {
    pub fn format_label<'a>(&'a self, id: &str, fallback: &'a str) -> &'a str {
        match id {
            "best" => self.format_best,
            _ => fallback,
        }
    }

    pub fn n_videos(&self, n: u64) -> String {
        let word = if n == 1 {
            self.video_one
        } else {
            self.video_many
        };
        format!("{n} {word}")
    }

    pub fn confirm_n(&self, n: u64) -> String {
        format!("{} {n}", self.confirm)
    }

    pub fn enqueue_n(&self, n: u64) -> String {
        format!("{} {n}", self.enqueue)
    }

    pub fn output_saved(&self, path: impl std::fmt::Display) -> String {
        format!("{} {path}", self.output_saved_prefix)
    }

    pub fn subtitles_on(&self, langs: &str) -> String {
        format!("--sub-langs {langs} · {}", self.subtitles_embedded)
    }

    pub fn browsers_found(&self, names: &str) -> String {
        format!(
            "{} {names} · {}",
            self.browsers_detected_prefix, self.browsers_detected_suffix
        )
    }

    pub fn theme_omarchy_named(&self, name: &str) -> String {
        format!("{} ({name})", self.theme_omarchy_prefix)
    }

    pub fn theme_named(&self, name: &str) -> String {
        format!("{} {name}", self.theme_named_prefix)
    }

    pub fn update_available(&self, version: &str) -> String {
        format!("{version} {}", self.update_available_suffix)
    }

    pub fn update_progress(&self, percent: u32) -> String {
        format!("{} {percent}%", self.update_downloading)
    }

    pub fn no_match(&self, query: &str) -> String {
        format!("{} \"{query}\"", self.no_match_prefix)
    }

    pub fn counted(&self, n: usize, one: &str, many: &str) -> String {
        format!("{n} {}", if n == 1 { one } else { many })
    }

    pub fn remaining_eta(&self, eta: &str) -> String {
        format!("{eta} {}", self.remaining)
    }

    pub fn retrying_in(&self, eta: &str) -> String {
        format!("{} · {eta} {}", self.status_retrying, self.remaining)
    }

    pub fn playlist_done(&self, n: usize) -> String {
        format!("{} {}", self.n_videos(n as u64), self.playlist_below)
    }

    pub fn notify_body(&self, done: usize, failed: usize) -> String {
        match (done, failed) {
            (0, n) => format!(
                "{} {}",
                self.counted(n, self.notify_download_one, self.notify_download_many),
                self.notify_failed_word
            ),
            (n, 0) => format!(
                "{} {}",
                self.counted(n, self.notify_download_one, self.notify_download_many),
                self.notify_done_word
            ),
            (ready, bad) => format!(
                "{ready} {}, {bad} {}",
                self.notify_mixed_ready, self.notify_mixed_failed
            ),
        }
    }

    pub fn consejo(&self, kind: crate::backend::ytdlp::Consejo) -> &'static str {
        match kind {
            crate::backend::ytdlp::Consejo::Forbidden => self.tip_forbidden,
            crate::backend::ytdlp::Consejo::Private => self.tip_private,
            crate::backend::ytdlp::Consejo::Unavailable => self.tip_unavailable,
            crate::backend::ytdlp::Consejo::FormatMissing => self.tip_format,
            crate::backend::ytdlp::Consejo::Timeout => self.tip_timeout,
            crate::backend::ytdlp::Consejo::NoMedia => self.tip_no_media,
        }
    }

    pub fn postprocessor(&self, name: &str) -> &str {
        match name {
            "Merger" => self.pp_merger,
            "ExtractAudio" => self.pp_extract,
            "EmbedThumbnail" => self.pp_thumb,
            _ => self.pp_generic,
        }
    }
}

/// Marca estable para el resumen de una lista expandida. La interfaz la
/// traduce; no es una ruta de archivo.
pub fn playlist_done_marker(count: usize) -> String {
    format!("playlist:{count}")
}

pub fn parse_playlist_done(path: &str) -> Option<usize> {
    path.strip_prefix("playlist:")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_ingles_es_el_default_y_el_respaldo() {
        assert_eq!(Language::default(), Language::En);
        assert_eq!(Language::parse(""), Language::En);
        assert_eq!(Language::parse("fr"), Language::En);
        assert_eq!(Language::parse("en"), Language::En);
        assert_eq!(Language::parse("ES"), Language::Es);
        assert_eq!(Language::parse("es-MX"), Language::Es);
    }

    #[test]
    fn el_catalogo_cambia_con_el_idioma() {
        assert_eq!(Language::En.catalog().settings, "settings");
        assert_eq!(Language::Es.catalog().settings, "ajustes");
        assert_eq!(Language::En.catalog().format_best, "Best");
        assert_eq!(Language::Es.catalog().format_best, "Mejor");
        assert_eq!(Language::Es.catalog().empty_title, "la cola está vacía");
    }

    #[test]
    fn serde_escribe_codigos_cortos_y_tolera_basura() {
        let json = serde_json::to_string(&Language::Es).expect("serde");
        assert_eq!(json, "\"es\"");
        let back: Language = serde_json::from_str("\"es\"").expect("es");
        assert_eq!(back, Language::Es);
        let unknown: Language = serde_json::from_str("\"pt\"").expect("pt");
        assert_eq!(unknown, Language::En);
    }

    #[test]
    fn las_formas_compuestas_respetan_el_idioma() {
        let en = Language::En.catalog();
        let es = Language::Es.catalog();
        assert_eq!(en.enqueue_n(19), "queue 19");
        assert_eq!(es.enqueue_n(19), "encolar 19");
        assert_eq!(en.n_videos(1), "1 video");
        assert_eq!(es.n_videos(3), "3 videos");
        assert_eq!(parse_playlist_done(&playlist_done_marker(4)), Some(4));
    }
}
