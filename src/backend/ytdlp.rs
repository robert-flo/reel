//! Los hilos que hablan con yt-dlp.
//!
//! Un trabajo, un hilo: la cola baja de a uno (`MAX_CONCURRENTES`), con pausa
//! entre videos. El hilo supervisor solo despacha ordenes y nunca espera a un
//! trabajo, asi que cancelar y encolar siguen andando mientras abajo se
//! descarga.
//!
//! Por trabajo, dos llamadas: `-J --no-playlist` para resolver titulo, autor y
//! duracion, y luego la descarga con `--newline --progress-template` para que
//! el progreso llegue en lineas faciles de leer en vez del dibujo de barra.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Proc, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{format_by_id, Command, Event, Kind, Media, Options, State};

/// Plantilla de progreso: una linea por actualizacion, campos separados por
/// `|`, sin nada que parsear a ojo.
const PROGRESS_TEMPLATE: &str =
    "download:PROGRESS|%(progress._percent_str)s|%(progress.speed)s|%(progress.eta)s";

/// Plantilla del postprocesado. yt-dlp la emite cuando empieza y termina cada
/// paso, asi que el estado se sabe por lo que dice yt-dlp y no por adivinar
/// con los textos que va imprimiendo. Antes esto era buscar "[Merger]" en la
/// salida: si yt-dlp cambiaba el texto, la fila dejaba de avisar y nadie se
/// enteraba.
const POSTPROCESS_TEMPLATE: &str =
    "postprocess:POSTPROCESS|%(progress.status)s|%(progress.postprocessor)s";

/// yt-dlp del PATH. Si algun dia se quiere el binario propio (como yoinks, que
/// lo baja a ~/.yoinks/bin), este es el unico lugar que cambia. `REEL_YTDLP`
/// apunta a otro, que es como se prueban las carreras sin bajar nada.
pub(crate) fn ytdlp_binary() -> PathBuf {
    std::env::var_os("REEL_YTDLP")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("yt-dlp"))
}

/// ffmpeg del PATH, con la misma salida que yt-dlp: `REEL_FFMPEG` apunta a
/// otro, que es como se prueba que pasa cuando falta.
pub(crate) fn ffmpeg_binary() -> PathBuf {
    std::env::var_os("REEL_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

/// Pregunta la version de una herramienta del sistema. Sirve para avisar al
/// arrancar y no cuando el usuario ya apreto "descargar".
///
/// `nombre` es lo que se le muestra a la persona y tambien el binario que se
/// busca en el PATH. `argumento` es como pide su version, que no es igual en
/// todas: yt-dlp usa `--version` y ffmpeg `-version`.
pub(crate) fn version_de(nombre: &str, binario: &Path, argumento: &str) -> Result<String, String> {
    let salida = Proc::new(binario)
        .arg(argumento)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => {
                format!("no encuentro {nombre}: instalalo (en Arch, `sudo pacman -S {nombre}`)")
            }
            _ => format!("no pude ejecutar {nombre}: {error}"),
        })?;

    if !salida.status.success() {
        return Err(format!(
            "{nombre} no contesto su version (salio con {}): revisa la instalacion",
            salida.status
        ));
    }

    let texto = String::from_utf8_lossy(&salida.stdout);
    let primera = texto.lines().next().unwrap_or("").trim().to_string();
    if primera.is_empty() {
        return Err(format!(
            "{nombre} no dijo su version: el binario del PATH no parece {nombre}"
        ));
    }
    Ok(primera)
}

/// La version de yt-dlp, o por que no se puede usar. Es el que baja todo.
pub(crate) fn version() -> Result<String, String> {
    version_de("yt-dlp", &ytdlp_binary(), "--version")
}

/// La version de ffmpeg, o por que no se puede usar.
///
/// Hace falta para unir pistas, extraer audio e incrustar metadatos, o sea que
/// sin el los formatos de video y `mp3` fallan al final. Vale avisarlo al
/// arrancar, junto con yt-dlp, y no cuando la descarga ya bajo 200 MB.
pub(crate) fn version_ffmpeg() -> Result<String, String> {
    // La primera linea es "ffmpeg version n9.0.2 Copyright (c) ..."; la parte
    // util es la version, no la linea entera.
    version_de("ffmpeg", &ffmpeg_binary(), "-version").map(|linea| {
        linea
            .strip_prefix("ffmpeg version ")
            .and_then(|resto| resto.split_whitespace().next())
            .unwrap_or(&linea)
            .to_string()
    })
}

fn default_dir(kind: Kind) -> PathBuf {
    let dirs = directories::UserDirs::new();
    let fallback = std::env::temp_dir();
    match kind {
        Kind::Audio => dirs
            .as_ref()
            .and_then(|d| d.audio_dir().map(PathBuf::from))
            .unwrap_or(fallback),
        Kind::Video => dirs
            .as_ref()
            .and_then(|d| d.video_dir().map(PathBuf::from))
            .unwrap_or(fallback),
    }
}

/// Los hijos vivos, para poder matarlos desde el supervisor mientras su hilo
/// espera. El `Mutex` se suelta antes de esperar, asi que nadie se queda
/// bloqueado detras de una descarga larga.
type Running = Arc<Mutex<HashMap<u64, Child>>>;

/// Cuantas lineas de stderr se guardan para poder contar por que fallo una
/// descarga. Acotado a proposito: con `--progress` son miles, y quedarse con
/// todas seria guardar el log entero en memoria por cada trabajo.
const LINEAS_DE_ERROR: usize = 40;

/// Las ultimas lineas de la salida de yt-dlp.
#[derive(Default)]
struct VecDeLineas(Vec<String>);

impl VecDeLineas {
    fn guardar(&mut self, line: String) {
        self.0.push(line);
        if self.0.len() > LINEAS_DE_ERROR {
            self.0.remove(0);
        }
    }

    fn texto(&self) -> String {
        self.0.join("\n")
    }
}

/// Cuantos trabajos pueden bajar a la vez. Siempre uno: bajar varios en
/// paralelo es lo que le saca el 403 a YouTube. Los que sobran quedan en
/// espera hasta que se libere el cupo.
pub const MAX_CONCURRENTES: usize = 1;

/// Pausa al azar entre un video y el siguiente, en milisegundos.
const PAUSA_ENTRE_VIDEOS_MS: (u64, u64) = (5_000, 15_000);

/// Espera antes de cada reintento automatico de un 403/429: 1 min, 3 min, 10 min.
const REINTENTO_MS: [u64; 3] = [60_000, 180_000, 600_000];

/// `--sleep-requests` por defecto: una pausa corta entre pedidos HTTP de yt-dlp.
const SLEEP_REQUESTS_SEGS: &str = "1";

/// Pausa entre videos. `REEL_PAUSA_MS=min,max` la achica en las pruebas.
pub(crate) fn pausa_entre_videos() -> (Duration, Duration) {
    parse_par_ms("REEL_PAUSA_MS").unwrap_or((
        Duration::from_millis(PAUSA_ENTRE_VIDEOS_MS.0),
        Duration::from_millis(PAUSA_ENTRE_VIDEOS_MS.1),
    ))
}

/// Las tres esperas de un 403/429. `REEL_REINTENTO_MS=a,b,c` las achica en las
/// pruebas para no esperar minutos de verdad.
pub(crate) fn esperas_de_reintento() -> [Duration; 3] {
    parse_triple_ms("REEL_REINTENTO_MS").unwrap_or([
        Duration::from_millis(REINTENTO_MS[0]),
        Duration::from_millis(REINTENTO_MS[1]),
        Duration::from_millis(REINTENTO_MS[2]),
    ])
}

/// Segundos entre pedidos HTTP de yt-dlp. `REEL_SLEEP_REQUESTS=0` lo apaga.
pub(crate) fn sleep_requests() -> Option<String> {
    let valor = std::env::var("REEL_SLEEP_REQUESTS").unwrap_or_else(|_| SLEEP_REQUESTS_SEGS.into());
    let recortado = valor.trim();
    if recortado.is_empty() || recortado == "0" {
        None
    } else {
        Some(recortado.to_string())
    }
}

/// La espera del reintento `intento` (0, 1, 2). `None` si ya no quedan.
pub(crate) fn espera_de_reintento(intento: u8) -> Option<Duration> {
    esperas_de_reintento().get(intento as usize).copied()
}

/// Un 403 o un 429 se reintenta solo; el resto queda en error de una.
pub(crate) fn es_reintentable(error: &str) -> bool {
    let texto = error.to_lowercase();
    texto.contains("403")
        || texto.contains("429")
        || texto.contains("forbidden")
        || texto.contains("too many requests")
}

fn parse_par_ms(nombre: &str) -> Option<(Duration, Duration)> {
    let crudo = std::env::var(nombre).ok()?;
    let mut partes = crudo.split(',');
    let min = partes.next()?.trim().parse::<u64>().ok()?;
    let max = partes.next()?.trim().parse::<u64>().ok()?;
    if partes.next().is_some() {
        return None;
    }
    Some((Duration::from_millis(min), Duration::from_millis(max)))
}

fn parse_triple_ms(nombre: &str) -> Option<[Duration; 3]> {
    let crudo = std::env::var(nombre).ok()?;
    let mut partes = crudo.split(',');
    let a = partes.next()?.trim().parse::<u64>().ok()?;
    let b = partes.next()?.trim().parse::<u64>().ok()?;
    let c = partes.next()?.trim().parse::<u64>().ok()?;
    if partes.next().is_some() {
        return None;
    }
    Some([
        Duration::from_millis(a),
        Duration::from_millis(b),
        Duration::from_millis(c),
    ])
}

fn pausa_al_azar(min: Duration, max: Duration) -> Duration {
    if max <= min {
        return min;
    }
    let span = (max - min).as_millis() as u64;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    min + Duration::from_millis(nanos % (span + 1))
}

/// Duerme a trozos para poder cancelar a mitad de una pausa larga.
fn dormir_cancelable(
    total: Duration,
    cancelled: &Mutex<HashSet<u64>>,
    id: u64,
    alive: &AtomicBool,
) -> bool {
    if total.is_zero() {
        return !lock(cancelled).contains(&id) && alive.load(Ordering::SeqCst);
    }
    let corte = Duration::from_millis(50);
    let inicio = Instant::now();
    while inicio.elapsed() < total {
        if lock(cancelled).contains(&id) || !alive.load(Ordering::SeqCst) {
            return false;
        }
        let queda = total.saturating_sub(inicio.elapsed());
        std::thread::sleep(corte.min(queda));
    }
    !lock(cancelled).contains(&id) && alive.load(Ordering::SeqCst)
}

/// El cupo de descargas simultaneas. Se pide antes de arrancar un trabajo y se
/// devuelve cuando termina. Pedirlo espera en el hilo del trabajo, no en el
/// supervisor: el resto de la cola sigue andando.
struct Cupos {
    libres: Mutex<usize>,
}

/// El cupo de un trabajo. Se devuelve solo, cuando el trabajo suelta esto, asi
/// que ningun camino de salida se puede olvidar de liberarlo.
struct Cupo<'a> {
    cupos: &'a Cupos,
}

impl Cupos {
    fn new(total: usize) -> Self {
        Self {
            libres: Mutex::new(total),
        }
    }

    /// Espera a que haya lugar, sin tope de tiempo. Mira cada pocos milisegundos
    /// en vez de dormir sobre una condicion: los cupos se liberan desde otros
    /// hilos y el trabajo sigue en espera hasta que toque.
    fn tomar(&self) -> Cupo<'_> {
        let espera = Duration::from_millis(5);
        loop {
            {
                let mut libres = lock(&self.libres);
                if *libres > 0 {
                    *libres -= 1;
                    return Cupo { cupos: self };
                }
            }
            std::thread::sleep(espera);
        }
    }
}

impl Drop for Cupo<'_> {
    fn drop(&mut self) {
        *lock(&self.cupos.libres) += 1;
    }
}

pub fn worker<W>(commands: Receiver<Command>, events: Sender<Event>, wake: W)
where
    W: Fn() + Send + Sync + 'static,
{
    let running: Running = Arc::new(Mutex::new(HashMap::new()));
    // Un id que llego a cancelarse. Sirve para dos cosas: marcar el trabajo, y
    // que su hilo no cuente despues un "listo" de algo que el usuario corto.
    let cancelled: Arc<Mutex<HashSet<u64>>> = Arc::new(Mutex::new(HashSet::new()));
    // En falso, un trabajo que termine durante el cierre ya no avisa nada.
    let alive = Arc::new(AtomicBool::new(true));
    let cupos = Arc::new(Cupos::new(MAX_CONCURRENTES));
    let wake = Arc::new(wake);
    let mut threads: Vec<JoinHandle<()>> = Vec::new();
    // El primer video no espera la pausa de 5-15 s; los siguientes si.
    let es_primera = Arc::new(AtomicBool::new(true));

    while let Ok(command) = commands.recv() {
        match command {
            Command::Shutdown => break,
            Command::Cancel { id } => {
                cancel(&running, &cancelled, &events, &*wake, id);
            }
            Command::Expandir { id, url } => {
                let (videos, error) = match entradas_de_lista(&url) {
                    Ok(videos) => (videos, None),
                    Err(reason) => (Vec::new(), Some(reason)),
                };
                let _ = events.send(Event::PlaylistExpandida { id, videos, error });
                wake();
            }
            Command::Preview { url } => {
                match probe(&url) {
                    Ok(media) => {
                        let _ = events.send(Event::Previewed { url, media });
                    }
                    Err(reason) => {
                        let _ = events.send(Event::PreviewFailed { reason });
                    }
                }
                wake();
            }
            Command::Start { id, url, options } => {
                // Nace en espera, no descargando: el estado dice la verdad
                // hasta que yt-dlp arranca de verdad, alla abajo.
                let _ = events.send(Event::StateChanged {
                    id,
                    state: State::Queued,
                });
                wake();

                let running = Arc::clone(&running);
                let cancelled = Arc::clone(&cancelled);
                let events_job = events.clone();
                let wake_job = Arc::clone(&wake);
                let alive = Arc::clone(&alive);
                let cupos = Arc::clone(&cupos);
                let es_primera = Arc::clone(&es_primera);
                // Se resuelve aca y no dentro del hilo: con varios trabajos a
                // la vez, leerlo alla seria una carrera entre todos.
                let ytdlp = ytdlp_binary();

                let handle = std::thread::Builder::new()
                    .name(format!("reel-job-{id}"))
                    .spawn(move || {
                        if lock(&cancelled).contains(&id) {
                            return;
                        }
                        // Esperar cupo es cosa del trabajo, no del supervisor:
                        // asi el resto de la cola sigue andando mientras tanto.
                        // Sin tope de tiempo: una lista larga no puede fallar
                        // solo por no entrar en su turno.
                        let _cupo = cupos.tomar();
                        if lock(&cancelled).contains(&id) {
                            return;
                        }
                        let saltar_pausa = es_primera.swap(false, Ordering::SeqCst);
                        if !saltar_pausa {
                            let (min, max) = pausa_entre_videos();
                            if !dormir_cancelable(pausa_al_azar(min, max), &cancelled, id, &alive) {
                                return;
                            }
                        }
                        if let Err(error) = atender_con_reintentos(
                            id,
                            &ytdlp,
                            &url,
                            &options,
                            &running,
                            &cancelled,
                            &events_job,
                            &wake_job,
                            &alive,
                        ) {
                            log::error!("no pude atender el trabajo {id}: {error}");
                        }
                    });

                match handle {
                    Ok(handle) => threads.push(handle),
                    Err(error) => {
                        let _ = events.send(Event::StateChanged {
                            id,
                            state: State::Failed {
                                reason: format!("no pude crear el hilo de descarga: {error}"),
                            },
                        });
                        wake();
                    }
                }
            }
        }
    }

    // Cierre: primero se mata todo, despues se espera. Al reves, un hijo vivo
    // dejaria a su hilo esperando para siempre.
    alive.store(false, Ordering::SeqCst);
    for (_, mut child) in lock(&running).drain() {
        let _ = child.kill();
    }
    for thread in threads {
        let _ = thread.join();
    }
}

/// Mata el hijo del trabajo y lo da por cancelado.
///
/// El orden importa: la marca va antes de sacarlo de la lista. Si el trabajo
/// estaba terminando justo ahora, sacarlo de la lista es la carrera que decide
/// quien reporta el final, y con la marca ya puesta el suyo no cuenta un
/// "listo" encima del "cancelado". Si ya no estaba, termino antes de que
/// llegara la orden y la marca no cambia nada.
///
/// Los ids marcados se acumulan mientras la app vive: son unos pocos bytes por
/// cancelacion, y borrarlos volveria a abrir justo la carrera que esto cierra.
fn cancel<W>(
    running: &Running,
    cancelled: &Mutex<HashSet<u64>>,
    events: &Sender<Event>,
    wake: &W,
    id: u64,
) where
    W: Fn(),
{
    lock(cancelled).insert(id);
    if let Some(mut child) = lock(running).remove(&id) {
        let _ = child.kill();
    }
    let _ = events.send(Event::StateChanged {
        id,
        state: State::Cancelled,
    });
    wake();
}

/// Un candado envenenado no invalida el dato: los eventos son independientes
/// entre si, asi que se sigue con lo que haya.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Lee los metadatos de yt-dlp. Separado de `probe` para poder probarlo con
/// JSON de verdad sin lanzar un proceso.
fn media_from_json(json: &serde_json::Value) -> Media {
    // Una url de playlist devuelve la playlist, no un video: el titulo es el
    // de la lista y no hay duracion. Se cuenta para poder avisarlo.
    let playlist_count = (json["_type"].as_str() == Some("playlist"))
        .then(|| json["playlist_count"].as_u64())
        .flatten();

    // En un video suelto la miniatura viene en `thumbnail`; en una playlist no,
    // asi que se usa la de la primera entrada.
    let thumbnail_url = json["thumbnail"]
        .as_str()
        .or_else(|| json["thumbnails"][0]["url"].as_str())
        .or_else(|| json["entries"][0]["thumbnail"].as_str())
        .map(str::to_string);

    let (duration, filesize) = if playlist_count.is_some() {
        let mut total_duration = 0.0;
        let mut total_filesize = 0u64;
        let mut tiene_duracion = false;
        let mut tiene_peso = false;

        if let Some(entries) = json["entries"].as_array() {
            for entry in entries {
                if let Some(d) = entry["duration"].as_f64() {
                    total_duration += d;
                    tiene_duracion = true;
                }
                if let Some(s) = entry["filesize"]
                    .as_u64()
                    .or_else(|| entry["filesize_approx"].as_u64())
                {
                    total_filesize += s;
                    tiene_peso = true;
                }
            }
        }

        let peso = if tiene_peso && total_filesize > 0 {
            Some(total_filesize)
        } else if tiene_duracion && total_duration > 0.0 {
            // Estimacion a ~2.5 Mbps para video 1080p (~312.5 KB/s)
            Some((total_duration * 312_500.0) as u64)
        } else {
            None
        };

        (tiene_duracion.then_some(total_duration), peso)
    } else {
        let duration = json["duration"].as_f64();
        let filesize = json["filesize"]
            .as_u64()
            .or_else(|| json["filesize_approx"].as_u64())
            .or_else(|| {
                json["formats"].as_array().and_then(|formats| {
                    formats
                        .iter()
                        .filter_map(|f| {
                            f["filesize"]
                                .as_u64()
                                .or_else(|| f["filesize_approx"].as_u64())
                        })
                        .max()
                })
            });
        (duration, filesize)
    };

    Media {
        title: json["title"].as_str().unwrap_or("").to_string(),
        uploader: json["uploader"]
            .as_str()
            .or_else(|| json["channel"].as_str())
            .unwrap_or("")
            .to_string(),
        duration,
        host: json["extractor_key"].as_str().unwrap_or("").to_lowercase(),
        thumbnail_url,
        playlist_count,
        // Vacia: la url del trabajo ya es esta.
        url: String::new(),
        filesize,
    }
}

pub(crate) fn probe(url: &str) -> Result<Media, String> {
    let output = Proc::new(ytdlp_binary())
        .args(["-J", "--no-playlist", "--no-warnings", url])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("no se pudo ejecutar yt-dlp: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(first_useful_line(&stderr));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("yt-dlp devolvio algo que no pude leer: {error}"))?;

    let is_playlist = json["_type"].as_str() == Some("playlist")
        || json["entries"].as_array().is_some_and(|e| !e.is_empty());
    let has_formats = json["formats"].as_array().is_some_and(|f| !f.is_empty())
        || json["url"].as_str().is_some()
        || json["direct"].as_bool().unwrap_or(false);

    if !is_playlist && !has_formats && json["duration"].as_f64().is_none() {
        return Err("no se encontraron archivos multimedia en el enlace".to_string());
    }

    Ok(media_from_json(&json))
}

/// Los videos que trae una lista, sin bajar nada: `--flat-playlist` pide solo
/// el listado, que es rapido y no trae los formatos.
///
/// Devuelve una `Media` por video, que es lo que necesita una fila de la cola.
/// La url de la lista se cambia por la de cada video, que es lo que hay que
/// pasarle despues a la descarga.
pub(crate) fn entradas_de_lista(url: &str) -> Result<Vec<Media>, String> {
    let output = Proc::new(ytdlp_binary())
        .args([
            "-J",
            "--flat-playlist",
            "--no-warnings",
            "--ignore-errors",
            url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("no se pudo ejecutar yt-dlp: {error}"))?;

    // Con `--ignore-errors` yt-dlp puede salir con error y aun asi traer
    // entradas; solo se falla si no hay ninguna que leer.
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        if output.status.success() {
            format!("yt-dlp devolvio algo que no pude leer: {error}")
        } else {
            first_useful_line(&String::from_utf8_lossy(&output.stderr))
        }
    })?;

    let entradas = json["entries"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    if entradas.is_empty() {
        return Err("la lista no trajo ningun video".into());
    }

    Ok(entradas.iter().filter_map(entrada_de_lista).collect())
}

/// Una entrada de `--flat-playlist` como `Media`. Los campos cambian de nombre
/// respecto de un video suelto: no hay `extractor_key` (viene `ie_key`), la
/// miniatura esta en `thumbnails`, y la url del video es `url`.
fn entrada_de_lista(json: &serde_json::Value) -> Option<Media> {
    let url = json["url"]
        .as_str()
        .or_else(|| json["webpage_url"].as_str())?;
    if url.is_empty() {
        return None;
    }

    let filesize = json["filesize"]
        .as_u64()
        .or_else(|| json["filesize_approx"].as_u64());

    Some(Media {
        title: json["title"].as_str().unwrap_or("").to_string(),
        uploader: json["uploader"]
            .as_str()
            .or_else(|| json["channel"].as_str())
            .unwrap_or("")
            .to_string(),
        duration: json["duration"].as_f64(),
        host: json["ie_key"]
            .as_str()
            .or_else(|| json["extractor_key"].as_str())
            .unwrap_or("")
            .to_lowercase(),
        thumbnail_url: json["thumbnails"][0]["url"]
            .as_str()
            .or_else(|| json["thumbnail"].as_str())
            .map(str::to_string),
        playlist_count: None,
        url: url.to_string(),
        filesize,
    })
}

/// Divide una cadena de argumentos respetando comillas simples y dobles.
pub(crate) fn parse_extra_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                } else {
                    current.push('\\');
                }
            }
            '"' | '\'' => {
                if in_quotes == Some(ch) {
                    in_quotes = None;
                } else if in_quotes.is_none() {
                    in_quotes = Some(ch);
                } else {
                    current.push(ch);
                }
            }
            c if c.is_whitespace() && in_quotes.is_none() => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// Los argumentos de la descarga, en orden. Aparte de `run_job` para poder
/// probarlos sin lanzar yt-dlp.
pub(crate) fn download_args(url: &str, options: &Options) -> (Vec<String>, PathBuf) {
    let format = format_by_id(&options.format_id);
    let dir = options
        .output_dir
        .clone()
        .unwrap_or_else(|| default_dir(format.kind));
    let template = options
        .filename_template
        .clone()
        .unwrap_or_else(|| crate::backend::DEFAULT_TEMPLATE.into());

    let mut args: Vec<String> = vec![
        "--newline".into(),
        "--progress".into(),
        "--no-warnings".into(),
        // Reanudar lo que quedo a medias. yt-dlp ya lo hace solo cuando
        // encuentra el `.part`, y cancelar lo deja ahi porque la carpeta y la
        // plantilla no cambian; se pide igual para que no dependa de un
        // default que podria cambiar.
        "--continue".into(),
        // El `.part` tiene que vivir en la misma carpeta que el archivo
        // final. Sin `temp:`, yt-dlp puede dejarlo en el cwd de reel; al
        // reabrir, `--continue` no lo encuentra y la fila no crece.
        "-P".into(),
        format!("temp:{}", dir.display()),
        "--progress-template".into(),
        PROGRESS_TEMPLATE.into(),
        "--progress-template".into(),
        POSTPROCESS_TEMPLATE.into(),
        "--no-playlist".into(),
        "-P".into(),
        dir.display().to_string(),
        "-o".into(),
        template,
        "--print".into(),
        "after_move:DONE|%(filepath)s".into(),
    ];

    if let Some(item) = options.playlist_item {
        args.push("--playlist-items".into());
        args.push(item.to_string());
    }

    // Bajar de nuevo aunque el archivo exista. Solo cuando se pidio: es la
    // recuperacion de un archivo truncado, no lo que hace un reintento normal.
    if options.force {
        args.push("--force-overwrites".into());
    } else {
        // Un archivo final que ya esta no se reescribe. `--continue` sigue
        // retomando el `.part`; esto solo cubre el caso listo.
        args.push("--no-overwrites".into());
    }

    if let Some(segundos) = sleep_requests() {
        args.push("--sleep-requests".into());
        args.push(segundos);
    }

    for arg in format.args {
        args.push((*arg).into());
    }
    if options.chapters && format.kind == Kind::Video {
        args.push("--embed-chapters".into());
    }
    if options.metadata {
        args.push("--embed-metadata".into());
        args.push("--embed-thumbnail".into());
    }
    if let Some(languages) = &options.subtitles {
        args.push("--write-subs".into());
        args.push("--write-auto-subs".into());
        args.push("--sub-langs".into());
        args.push(languages.clone());
        args.push("--embed-subs".into());
    }
    if let Some(browser) = &options.cookies_from_browser {
        args.push("--cookies-from-browser".into());
        args.push(browser.clone());
    }
    if let Some(rate_limit) = &options.rate_limit {
        args.push("--limit-rate".into());
        args.push(rate_limit.clone());
    }
    if options.sponsorblock && format.kind == Kind::Video {
        args.push("--sponsorblock-remove".into());
        args.push("sponsor".into());
    }
    if let Some(extra) = &options.extra_args {
        for part in parse_extra_args(extra) {
            args.push(part);
        }
    }
    if let Some(sections) = &options.download_sections {
        args.push("--download-sections".into());
        args.push(sections.clone());
        args.push("--force-keyframes-at-cuts".into());
    }
    args.push(url.into());

    (args, dir)
}

#[allow(clippy::too_many_arguments)]
fn atender_con_reintentos<W>(
    id: u64,
    ytdlp: &Path,
    url: &str,
    options: &Options,
    running: &Running,
    cancelled: &Mutex<HashSet<u64>>,
    events: &Sender<Event>,
    wake: &Arc<W>,
    alive: &AtomicBool,
) -> std::io::Result<()>
where
    W: Fn() + Send + Sync + 'static,
{
    let mut fallos: u8 = 0;
    loop {
        let estado = run_job(
            id, ytdlp, url, options, running, cancelled, events, wake, alive,
        )?;
        let Some(estado) = estado else {
            return Ok(());
        };
        match estado {
            State::Failed { reason } if es_reintentable(&reason) => {
                if let Some(espera) = espera_de_reintento(fallos) {
                    fallos += 1;
                    let _ = events.send(Event::StateChanged {
                        id,
                        state: State::Retrying {
                            reason: reason.clone(),
                            attempt: fallos,
                            wait_ms: espera.as_millis() as u64,
                        },
                    });
                    wake();
                    if !dormir_cancelable(espera, cancelled, id, alive) {
                        return Ok(());
                    }
                    continue;
                }
                let _ = events.send(Event::StateChanged {
                    id,
                    state: State::Failed { reason },
                });
                wake();
                return Ok(());
            }
            otro => {
                let _ = events.send(Event::StateChanged { id, state: otro });
                wake();
                return Ok(());
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_job<W>(
    id: u64,
    ytdlp: &Path,
    url: &str,
    options: &Options,
    running: &Running,
    cancelled: &Mutex<HashSet<u64>>,
    events: &Sender<Event>,
    wake: &Arc<W>,
    alive: &AtomicBool,
) -> std::io::Result<Option<State>>
where
    W: Fn() + Send + Sync + 'static,
{
    if lock(cancelled).contains(&id) || !alive.load(Ordering::SeqCst) {
        return Ok(None);
    }

    let (args, dir) = download_args(url, options);

    let mut child = Proc::new(ytdlp)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    lock(running).insert(id, child);

    if lock(cancelled).contains(&id) || !alive.load(Ordering::SeqCst) {
        if let Some(mut child) = lock(running).remove(&id) {
            let _ = child.kill();
        }
        return Ok(None);
    }

    // Ya hay un yt-dlp corriendo para este trabajo: ahora si.
    let _ = events.send(Event::StateChanged {
        id,
        state: State::Downloading,
    });
    wake();

    // Las dos tuberias se leen en su propio hilo, y las dos pasan por el mismo
    // clasificador. yt-dlp manda el progreso Y los avisos de postprocesado por
    // stderr, no por stdout: leer solo stdout era leer media conversacion.
    //
    // Ademas, si nadie leyera una tuberia, su buffer se llena y yt-dlp se queda
    // esperando: las dos partes trabadas.
    //
    // Lo de stderr tambien se guarda, acotado, para poder contar por que fallo.
    let collected = Arc::new(Mutex::new(VecDeLineas::default()));
    let final_path: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let mut readers: Vec<JoinHandle<()>> = Vec::new();
    let tuberias: [(Option<Box<dyn Read + Send>>, bool); 2] = [
        (stdout.map(|s| Box::new(s) as Box<dyn Read + Send>), false),
        (stderr.map(|s| Box::new(s) as Box<dyn Read + Send>), true),
    ];
    for (stream, guardar) in tuberias {
        let Some(stream) = stream else { continue };
        let events = events.clone();
        let final_path = Arc::clone(&final_path);
        let collected = Arc::clone(&collected);
        let wake = Arc::clone(wake);
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                match leer_linea(id, &line) {
                    Salida::Progreso(event) => {
                        let _ = events.send(event);
                        wake();
                    }
                    Salida::Postprocesado(postprocessor) => {
                        let _ = events.send(Event::StateChanged {
                            id,
                            state: State::Postprocessing { postprocessor },
                        });
                        wake();
                    }
                    Salida::Terminado(path) => {
                        *lock(&final_path) = Some(path);
                    }
                    Salida::Nada => {}
                }
                if guardar {
                    lock(&collected).guardar(line);
                }
            }
        }));
    }

    // El candado se suelta antes de esperar: si esperara con el puesto, una
    // cancelacion no podria ni entrar a matar el hijo.
    let child = lock(running).remove(&id);
    let status = child.map(|mut child| child.wait());
    // Se espera a los lectores antes de mirar lo que juntaron.
    for reader in readers {
        let _ = reader.join();
    }
    let final_path = lock(&final_path).clone();

    // La marca de cancelado es la fuente de verdad, y `cancel` la pone antes de
    // matar el hijo. Asi, cuando este hilo mira, ya esta: nunca reporta un
    // "listo" encima de un "cancelado". Durante el cierre, ademas, no hay a
    // quien contarle nada.
    if lock(cancelled).contains(&id) || !alive.load(Ordering::SeqCst) {
        return Ok(None);
    }

    let state = match status {
        Some(Ok(status)) if status.success() => State::Done {
            path: final_path.unwrap_or_else(|| dir.display().to_string()),
        },
        Some(Ok(_)) => State::Failed {
            reason: first_useful_line(&lock(&collected).texto()),
        },
        Some(Err(error)) => State::Failed {
            reason: format!("no pude esperar a yt-dlp: {error}"),
        },
        // Sin hijo que esperar y sin marca de cancelado: no deberia pasar, pero
        // mas vale decirlo que quedarse callado.
        None => State::Failed {
            reason: "la descarga se interrumpio".into(),
        },
    };

    Ok(Some(state))
}

/// Que hacer con una linea de la salida de yt-dlp. Separado del bucle para
/// poder probarlo: es donde vive el contrato con yt-dlp, y antes se equivocaba
/// en silencio.
#[derive(Debug)]
pub(crate) enum Salida {
    /// Una actualizacion de progreso.
    Progreso(Event),
    /// El aviso de que arranco un paso de postprocesado.
    Postprocesado(String),
    /// La ruta final, cuando yt-dlp ya movio el archivo.
    Terminado(String),
    /// Lo que no nos dice nada.
    Nada,
}

pub(crate) fn leer_linea(id: u64, line: &str) -> Salida {
    if let Some(rest) = line.strip_prefix("PROGRESS|") {
        return match parse_progress(id, rest) {
            Some(event) => Salida::Progreso(event),
            None => Salida::Nada,
        };
    }
    if let Some(path) = line.strip_prefix("DONE|") {
        return Salida::Terminado(path.trim().to_string());
    }
    if let Some(rest) = line.strip_prefix("POSTPROCESS|") {
        return match parse_postprocess(rest) {
            Some(postprocessor) => Salida::Postprocesado(postprocessor),
            None => Salida::Nada,
        };
    }
    Salida::Nada
}

/// De que paso del postprocesado avisa yt-dlp. `None` cuando no hay que contarlo.
///
/// Solo los pasos que tardan de verdad: unir pistas, extraer el audio y poner
/// la caratula. Los demas —los metadatos, mover el archivo— pasan en un
/// parpadeo, y avisarlos solo haria saltar la fila sin que nadie alcance a
/// leerla. Se averiguo mirando lo que emite yt-dlp de verdad, no a ojo.
fn parse_postprocess(rest: &str) -> Option<String> {
    let mut parts = rest.split('|');
    let status = parts.next()?.trim();
    let postprocessor = parts.next()?.trim();

    if status != "started" || postprocessor.is_empty() {
        return None;
    }
    matches!(postprocessor, "Merger" | "ExtractAudio" | "EmbedThumbnail")
        .then(|| postprocessor.to_string())
}

pub(crate) fn parse_progress(id: u64, rest: &str) -> Option<Event> {
    let mut parts = rest.split('|');
    let percent = parts.next()?.trim().trim_end_matches('%');
    let speed = parts.next().unwrap_or("NA").trim();
    let eta = parts.next().unwrap_or("NA").trim();

    let progress = percent.parse::<f32>().ok()? / 100.0;
    Some(Event::Progress {
        id,
        progress: progress.clamp(0.0, 1.0),
        speed: speed.parse::<f64>().ok(),
        eta_secs: eta.parse::<u64>().ok(),
    })
}

/// Un error de yt-dlp que se repite, ya clasificado. El texto se traduce
/// en la interfaz: aca solo se reconoce el caso.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consejo {
    Forbidden,
    Private,
    Unavailable,
    FormatMissing,
    Timeout,
    NoMedia,
}

/// Que hacer con un error que se repite. Devuelve `None` cuando no hay nada
/// util que agregar, y ahi se deja el mensaje de yt-dlp tal cual.
///
/// El 403 de YouTube, por ejemplo, no es culpa del usuario ni del archivo: es
/// que el sitio frena los pedidos seguidos. Decir eso ahorra el rato de pensar
/// que la app esta rota.
pub(crate) fn consejo_para(error: &str) -> Option<Consejo> {
    let texto = error.to_lowercase();

    if texto.contains("403")
        || texto.contains("forbidden")
        || texto.contains("429")
        || texto.contains("too many requests")
    {
        return Some(Consejo::Forbidden);
    }
    if texto.contains("private video") || texto.contains("login") || texto.contains("sign in") {
        return Some(Consejo::Private);
    }
    if texto.contains("video unavailable") || texto.contains("removed") {
        return Some(Consejo::Unavailable);
    }
    if texto.contains("requested format is not available") {
        return Some(Consejo::FormatMissing);
    }
    if texto.contains("timed out") || texto.contains("timeout") {
        return Some(Consejo::Timeout);
    }
    if texto.contains("unsupported url")
        || texto.contains("no video")
        || texto.contains("no media")
        || texto.contains("there's no video")
        || texto.contains("does not contain any video")
        || texto.contains("doesn't contain any video")
        || texto.contains("not a valid url")
        || texto.contains("is not a valid url")
        || texto.contains("none of the url")
        || texto.contains("no se encontraron archivos multimedia")
    {
        return Some(Consejo::NoMedia);
    }
    None
}

/// yt-dlp escupe varias lineas; la primera que empieza con ERROR es la util.
fn first_useful_line(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| line.contains("ERROR"))
        .or_else(|| stderr.lines().find(|line| !line.trim().is_empty()))
        .unwrap_or("yt-dlp fallo sin decir por que")
        .trim()
        .trim_start_matches("ERROR: ")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn lee_una_linea_de_progreso() {
        let event = parse_progress(7, " 42.5%|1048576.0|83").expect("deberia parsear");
        match event {
            Event::Progress {
                id,
                progress,
                speed,
                eta_secs,
            } => {
                assert_eq!(id, 7);
                assert!((progress - 0.425).abs() < 0.001);
                assert_eq!(speed, Some(1048576.0));
                assert_eq!(eta_secs, Some(83));
            }
            other => panic!("evento inesperado: {other:?}"),
        }
    }

    #[test]
    fn tolera_campos_vacios() {
        let event = parse_progress(1, "  0.0%|NA|NA").expect("deberia parsear");
        match event {
            Event::Progress {
                speed, eta_secs, ..
            } => {
                assert_eq!(speed, None);
                assert_eq!(eta_secs, None);
            }
            other => panic!("evento inesperado: {other:?}"),
        }
    }

    #[test]
    fn saca_la_linea_de_error() {
        let stderr = "WARNING: algo\nERROR: Requested format is not available\n";
        assert_eq!(
            first_useful_line(stderr),
            "Requested format is not available"
        );
    }

    #[test]
    fn sin_error_legible_devuelve_la_primera_linea() {
        let stderr = "\nWARNING: algo raro\n";
        assert_eq!(first_useful_line(stderr), "WARNING: algo raro");
        assert_eq!(first_useful_line("   "), "yt-dlp fallo sin decir por que");
    }

    #[test]
    fn la_descarga_lleva_la_carpeta_y_la_plantilla() {
        let options = Options {
            output_dir: Some(PathBuf::from("/tmp/reel-destino")),
            filename_template: Some("%(id)s.%(ext)s".into()),
            ..Options::default()
        };
        let (args, dir) = download_args("https://ejemplo.test/v", &options);

        assert_eq!(dir, PathBuf::from("/tmp/reel-destino"));
        assert!(args.iter().any(|arg| arg == "/tmp/reel-destino"));
        assert!(args.iter().any(|arg| arg == "%(id)s.%(ext)s"));
        // La url va ultima, que es como la espera yt-dlp.
        assert_eq!(
            args.last().map(String::as_str),
            Some("https://ejemplo.test/v")
        );
    }

    /// El cupo no deja pasar a mas trabajos de los que permite, y devuelve el
    /// lugar al soltarlo.
    #[test]
    fn el_cupo_limita_los_que_corren_a_la_vez() {
        const HILOS: usize = 8;
        const CUPO: usize = 3;

        let cupos = Arc::new(Cupos::new(CUPO));
        let dentro = Arc::new(AtomicUsize::new(0));
        let maximo = Arc::new(AtomicUsize::new(0));
        let soltados = Arc::new(AtomicUsize::new(0));

        let mut hilos = Vec::new();
        for _ in 0..HILOS {
            let cupos = Arc::clone(&cupos);
            let dentro = Arc::clone(&dentro);
            let maximo = Arc::clone(&maximo);
            let soltados = Arc::clone(&soltados);
            hilos.push(std::thread::spawn(move || {
                let cupo = cupos.tomar();
                let ahora = dentro.fetch_add(1, Ordering::SeqCst) + 1;
                maximo.fetch_max(ahora, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(30));
                dentro.fetch_sub(1, Ordering::SeqCst);
                drop(cupo);
                soltados.fetch_add(1, Ordering::SeqCst);
            }));
        }
        for hilo in hilos {
            let _ = hilo.join();
        }

        assert_eq!(soltados.load(Ordering::SeqCst), HILOS);
        assert!(
            maximo.load(Ordering::SeqCst) <= CUPO,
            "llegaron a correr {} a la vez con un cupo de {CUPO}",
            maximo.load(Ordering::SeqCst)
        );
        assert_eq!(*lock(&cupos.libres), CUPO, "los cupos no volvieron todos");
    }

    /// Si el unico lugar esta ocupado, el que llega despues espera hasta que
    /// se libere: no se rinde a los pocos milisegundos.
    #[test]
    fn el_cupo_espera_hasta_que_haya_lugar() {
        let cupos = Arc::new(Cupos::new(1));
        let ocupado = cupos.tomar();
        let listo = Arc::new(AtomicBool::new(false));
        let cupos_espera = Arc::clone(&cupos);
        let listo_espera = Arc::clone(&listo);

        let hilo = std::thread::spawn(move || {
            let _cupo = cupos_espera.tomar();
            listo_espera.store(true, Ordering::SeqCst);
        });

        std::thread::sleep(Duration::from_millis(80));
        assert!(
            !listo.load(Ordering::SeqCst),
            "no deberia haber tomado el cupo mientras sigue ocupado"
        );
        drop(ocupado);
        hilo.join().expect("el que espera deberia terminar");
        assert!(
            listo.load(Ordering::SeqCst),
            "al soltar el cupo, el que espera tiene que tomarlo"
        );
        assert_eq!(*lock(&cupos.libres), 1, "el cupo no volvio");
    }

    /// El postprocesado se detecta por el aviso estructurado de yt-dlp.
    #[test]
    fn lee_el_aviso_de_postprocesado() {
        assert_eq!(
            parse_postprocess("started|Merger"),
            Some("Merger".to_string())
        );
        assert_eq!(
            parse_postprocess("started|ExtractAudio"),
            Some("ExtractAudio".to_string())
        );
    }

    /// La caratula tambien tarda y tambien se cuenta: es lo que hace una
    /// descarga de audio con `--embed-thumbnail`.
    #[test]
    fn cuenta_la_caratula() {
        assert_eq!(
            parse_postprocess("started|EmbedThumbnail"),
            Some("EmbedThumbnail".to_string())
        );
    }

    /// Lo que no hay que contar: el final de un paso, los pasos que pasan en
    /// un parpadeo, y un aviso sin nombre. `Metadata` y `MoveFiles` salen en
    /// cualquier descarga con metadatos: no son trabajo que valga anunciar.
    #[test]
    fn ignora_lo_que_no_es_un_paso_que_contar() {
        assert_eq!(parse_postprocess("finished|Merger"), None);
        assert_eq!(parse_postprocess("started|MoveFiles"), None);
        assert_eq!(parse_postprocess("started|Metadata"), None);
        assert_eq!(parse_postprocess("started|"), None);
        assert_eq!(parse_postprocess(""), None);
        assert_eq!(parse_postprocess("started"), None);
    }

    /// Esta es la razon de todo el cambio: el texto suelto que antes movia el
    /// estado ya no lo mueve, porque ahora se mira el aviso de yt-dlp y no lo
    /// que se le ocurra imprimir. Si yt-dlp cambia ese texto, la fila sigue
    /// avisando.
    #[test]
    fn el_texto_suelto_ya_no_mueve_el_estado() {
        let lineas = [
            "[Merger] Merging formats into \"salida.mp4\"",
            "[ExtractAudio] Destination: salida.mp3",
            "Merging formats into salida.mp4",
        ];
        for line in lineas {
            assert!(
                matches!(leer_linea(1, line), Salida::Nada),
                "{line:?} no deberia mover el estado"
            );
        }
    }

    /// Cada linea de yt-dlp cae donde tiene que caer.
    #[test]
    fn reparte_las_lineas_de_ytdlp() {
        assert!(matches!(
            leer_linea(3, "PROGRESS| 50%|1024.0|10"),
            Salida::Progreso(Event::Progress { id: 3, .. })
        ));
        assert!(matches!(
            leer_linea(3, "POSTPROCESS|started|Merger"),
            Salida::Postprocesado(paso) if paso == "Merger"
        ));
        assert!(matches!(
            leer_linea(3, "DONE|/tmp/video.mp4"),
            Salida::Terminado(ruta) if ruta == "/tmp/video.mp4"
        ));
        assert!(matches!(leer_linea(3, "cualquier cosa"), Salida::Nada));
        // Un progreso que no se puede leer tampoco inventa nada.
        assert!(matches!(leer_linea(3, "PROGRESS|NA|NA|NA"), Salida::Nada));
    }

    /// La descarga pide las dos plantillas: la del progreso y la del
    /// postprocesado. Sin la segunda, la fila nunca diria que esta esperando
    /// ffmpeg.
    /// Un video suelto se lee como siempre.
    #[test]
    fn lee_un_video_suelto() {
        let json = serde_json::json!({
            "title": "Un video",
            "uploader": "alguien",
            "duration": 213.0,
            "extractor_key": "Youtube",
            "thumbnail": "https://ejemplo.test/mini.jpg"
        });
        let media = media_from_json(&json);
        assert_eq!(media.title, "Un video");
        assert_eq!(media.uploader, "alguien");
        assert_eq!(media.duration, Some(213.0));
        assert_eq!(media.host, "youtube");
        assert_eq!(media.playlist_count, None);
        assert_eq!(
            media.thumbnail_url.as_deref(),
            Some("https://ejemplo.test/mini.jpg")
        );
    }

    /// Una playlist no es un video: se marca cuantos trae, no se inventa una
    /// duracion (la de la lista no significa nada) y la miniatura sale de la
    /// primera entrada, que es lo unico que yt-dlp da.
    #[test]
    fn reconoce_una_playlist() {
        let json = serde_json::json!({
            "_type": "playlist",
            "title": "Una lista",
            "uploader": "alguien",
            "playlist_count": 19,
            "extractor_key": "Youtube",
            "entries": [{ "thumbnail": "https://ejemplo.test/primera.jpg" }]
        });
        let media = media_from_json(&json);
        assert_eq!(media.playlist_count, Some(19));
        assert_eq!(
            media.duration, None,
            "la duracion de una lista no es de un video"
        );
        assert_eq!(media.title, "Una lista");
        assert_eq!(
            media.thumbnail_url.as_deref(),
            Some("https://ejemplo.test/primera.jpg")
        );
    }

    /// Una entrada de `--flat-playlist` usa otros nombres de campo que un video
    /// suelto: `ie_key` en vez de `extractor_key`, `thumbnails` en vez de
    /// `thumbnail`, y trae la url del video en `url`.
    #[test]
    fn lee_una_entrada_de_lista() {
        let json = serde_json::json!({
            "title": "Un video de la lista",
            "url": "https://www.youtube.com/watch?v=abc123",
            "duration": 969,
            "uploader": "alguien",
            "ie_key": "Youtube",
            "thumbnails": [{ "url": "https://ejemplo.test/mini.jpg" }]
        });
        let media = entrada_de_lista(&json).expect("deberia leerla");
        assert_eq!(media.url, "https://www.youtube.com/watch?v=abc123");
        assert_eq!(media.title, "Un video de la lista");
        assert_eq!(media.duration, Some(969.0));
        assert_eq!(media.host, "youtube");
        assert_eq!(
            media.thumbnail_url.as_deref(),
            Some("https://ejemplo.test/mini.jpg")
        );
        assert_eq!(media.playlist_count, None);
    }

    /// Sin url no hay nada que bajar, asi que la entrada se descarta en vez de
    /// crear una fila que no puede funcionar.
    #[test]
    fn descarta_una_entrada_sin_url() {
        let json = serde_json::json!({ "title": "sin url" });
        assert!(entrada_de_lista(&json).is_none());

        let vacia = serde_json::json!({ "title": "url vacia", "url": "" });
        assert!(entrada_de_lista(&vacia).is_none());
    }

    /// La version de ffmpeg viene dentro de una linea larga: hay que sacarle
    /// el numero, no mostrar el aviso de copyright entero.
    #[test]
    fn saca_la_version_de_ffmpeg_de_su_linea() {
        let linea = "ffmpeg version n9.0.2 Copyright (c) 2000-2026 the FFmpeg developers";
        let version = linea
            .strip_prefix("ffmpeg version ")
            .and_then(|resto| resto.split_whitespace().next())
            .unwrap_or(linea);
        assert_eq!(version, "n9.0.2");
    }

    /// Los errores que se repiten tienen que decir que hacer, no solo que
    /// fallaron. El 403 de YouTube salio en una prueba de verdad.
    #[test]
    fn los_errores_conocidos_traen_consejo() {
        let real = "unable to download video data: HTTP Error 403: Forbidden";
        assert_eq!(consejo_para(real), Some(Consejo::Forbidden));
        assert_eq!(
            consejo_para("HTTP Error 429: Too Many Requests"),
            Some(Consejo::Forbidden)
        );

        assert_eq!(consejo_para("ERROR: Private video"), Some(Consejo::Private));
        assert_eq!(
            consejo_para("Video unavailable"),
            Some(Consejo::Unavailable)
        );
        assert_eq!(
            consejo_para("Requested format is not available"),
            Some(Consejo::FormatMissing)
        );
        assert_eq!(consejo_para("connection timed out"), Some(Consejo::Timeout));
        assert_eq!(
            consejo_para("ERROR: Unsupported URL: https://x.com/paolino"),
            Some(Consejo::NoMedia)
        );
        assert_eq!(
            consejo_para("ERROR: [twitter] 20: No video could be found in this tweet"),
            Some(Consejo::NoMedia)
        );
        assert_eq!(consejo_para("No media found"), Some(Consejo::NoMedia));
        assert_eq!(
            consejo_para("no se encontraron archivos multimedia en el enlace"),
            Some(Consejo::NoMedia)
        );
    }

    /// Un error que no conocemos se deja como vino: inventar un consejo seria
    /// peor que no dar ninguno.
    #[test]
    fn un_error_desconocido_no_inventa_consejo() {
        assert_eq!(consejo_para("algo raro paso"), None);
        assert_eq!(consejo_para(""), None);
    }

    /// Cancelar deja el `.part` en la carpeta y el reintento vuelve a pedir la
    /// misma, asi que yt-dlp reanuda donde iba. Se pide explicito para no
    /// depender de que su default siga siendo reanudar.
    #[test]
    fn la_descarga_puede_reanudar() {
        let options = Options::default();
        let (args, _) = download_args("https://ejemplo.test/v", &options);
        assert!(args.iter().any(|arg| arg == "--continue"));
    }

    /// Un archivo que ya esta no se pisa: `--no-overwrites` junto a
    /// `--continue` saltea el final y retoma el `.part`.
    #[test]
    fn no_reescribe_un_archivo_que_existe() {
        let options = Options::default();
        let (args, dir) = download_args("https://ejemplo.test/v", &options);
        assert!(args.iter().any(|arg| arg == "--no-overwrites"));
        assert!(args.iter().any(|arg| arg == "--continue"));
        let temp = format!("temp:{}", dir.display());
        assert!(
            args.iter().any(|arg| arg == &temp),
            "el .part tiene que ir a la carpeta de salida, no al cwd: {args:?}"
        );
        assert!(args.iter().any(|arg| arg == "--sleep-requests"));
    }

    #[test]
    fn siempre_baja_de_a_uno() {
        assert_eq!(MAX_CONCURRENTES, 1);
    }

    #[test]
    fn un_403_o_429_se_reintenta_con_espera_creciente() {
        assert!(es_reintentable("HTTP Error 403: Forbidden"));
        assert!(es_reintentable("HTTP Error 429: Too Many Requests"));
        assert!(!es_reintentable("Requested format is not available"));

        let esperas = esperas_de_reintento();
        assert_eq!(esperas[0], Duration::from_secs(60));
        assert_eq!(esperas[1], Duration::from_secs(180));
        assert_eq!(esperas[2], Duration::from_secs(600));
        assert!(espera_de_reintento(0).is_some());
        assert!(espera_de_reintento(1).is_some());
        assert!(espera_de_reintento(2).is_some());
        assert!(espera_de_reintento(3).is_none());
    }

    /// Bajar de cero es una salida explicita, no lo que pasa siempre: pedirla
    /// fuerza la bandera, y sin pedirla no aparece.
    #[test]
    fn solo_baja_de_cero_cuando_se_pide() {
        let normal = Options::default();
        let (args, _) = download_args("https://ejemplo.test/v", &normal);
        assert!(!args.iter().any(|arg| arg == "--force-overwrites"));

        let forzado = Options {
            force: true,
            ..Options::default()
        };
        let (args, _) = download_args("https://ejemplo.test/v", &forzado);
        assert!(args.iter().any(|arg| arg == "--force-overwrites"));
        assert!(!args.iter().any(|arg| arg == "--no-overwrites"));
        // Y sigue pudiendo reanudar lo que quedo a medias.
        assert!(args.iter().any(|arg| arg == "--continue"));
    }

    #[test]
    fn la_descarga_pide_las_dos_plantillas() {
        let options = Options::default();
        let (args, _) = download_args("https://ejemplo.test/v", &options);
        let plantillas = args
            .iter()
            .enumerate()
            .filter(|(_, arg)| arg.as_str() == "--progress-template")
            .count();
        assert_eq!(plantillas, 2, "faltan plantillas en {args:?}");
        assert!(args.iter().any(|arg| arg == POSTPROCESS_TEMPLATE));
        assert!(args.iter().any(|arg| arg == PROGRESS_TEMPLATE));
    }

    /// El formato de audio ya trae sus propios `--embed-*`, asi que la casilla
    /// de metadatos no tiene que repetirlos: dos veces el mismo flag es ruido.
    #[test]
    fn los_flags_de_embed_no_se_repiten() {
        for format_id in ["mp3", "opus"] {
            let options = Options {
                format_id: format_id.into(),
                metadata: true,
                ..Options::default()
            };
            let (args, _) = download_args("https://ejemplo.test/v", &options);
            for flag in ["--embed-metadata", "--embed-thumbnail"] {
                let count = args.iter().filter(|arg| *arg == flag).count();
                assert!(count <= 1, "{flag} aparece {count} veces en {format_id}");
            }
        }
    }

    #[test]
    fn la_descarga_pide_subtitulos_manuales_y_automaticos() {
        let options = Options {
            subtitles: Some("es,en".into()),
            ..Options::default()
        };
        let (args, _) = download_args("https://ejemplo.test/v", &options);
        assert!(args.iter().any(|arg| arg == "--write-subs"));
        assert!(args.iter().any(|arg| arg == "--write-auto-subs"));
        assert!(args.iter().any(|arg| arg == "--embed-subs"));
        assert!(args.iter().any(|arg| arg == "es,en"));
    }

    #[test]
    fn la_descarga_lleva_limite_de_velocidad() {
        let options = Options {
            rate_limit: Some("5M".into()),
            ..Options::default()
        };
        let (args, _) = download_args("https://ejemplo.test/v", &options);
        assert!(args.iter().any(|arg| arg == "--limit-rate"));
        assert!(args.iter().any(|arg| arg == "5M"));
    }

    #[test]
    fn la_descarga_lleva_sponsorblock() {
        let options = Options {
            sponsorblock: true,
            ..Options::default()
        };
        let (args, _) = download_args("https://ejemplo.test/v", &options);
        assert!(args.iter().any(|arg| arg == "--sponsorblock-remove"));
        assert!(args.iter().any(|arg| arg == "sponsor"));

        // En audio no debe agregarlo
        let options_audio = Options {
            format_id: "mp3".into(),
            sponsorblock: true,
            ..Options::default()
        };
        let (args_audio, _) = download_args("https://ejemplo.test/v", &options_audio);
        assert!(!args_audio.iter().any(|arg| arg == "--sponsorblock-remove"));
    }

    #[test]
    fn lee_video_con_duracion_y_peso() {
        let json = serde_json::json!({
            "title": "Un video",
            "uploader": "Alguien",
            "duration": 120.0,
            "extractor_key": "youtube",
            "filesize": 1048576,
        });
        let media = media_from_json(&json);
        assert_eq!(media.title, "Un video");
        assert_eq!(media.duration, Some(120.0));
        assert_eq!(media.filesize, Some(1048576));
    }

    #[test]
    fn parse_extra_args_respeta_comillas_y_espacios() {
        let parsed = parse_extra_args("--proxy socks5://127.0.0.1:9050 --geo-bypass");
        assert_eq!(
            parsed,
            vec!["--proxy", "socks5://127.0.0.1:9050", "--geo-bypass"]
        );

        let parsed_quotes =
            parse_extra_args("--user-agent \"Mozilla 5.0\" --referer 'https://test'");
        assert_eq!(
            parsed_quotes,
            vec!["--user-agent", "Mozilla 5.0", "--referer", "https://test"]
        );

        let parsed_vacio = parse_extra_args("   ");
        assert!(parsed_vacio.is_empty());
    }

    #[test]
    fn la_descarga_lleva_extra_args() {
        let options = Options {
            extra_args: Some("--geo-bypass --proxy socks5://127.0.0.1:9050".into()),
            ..Options::default()
        };
        let (args, _) = download_args("https://ejemplo.test/v", &options);
        assert!(args.iter().any(|arg| arg == "--geo-bypass"));
        assert!(args.iter().any(|arg| arg == "--proxy"));
        assert!(args.iter().any(|arg| arg == "socks5://127.0.0.1:9050"));
    }

    #[test]
    fn format_download_section_arma_los_rangos() {
        use crate::backend::format_download_section;
        assert_eq!(
            format_download_section("01:30", "03:45"),
            Some("*01:30-03:45".into())
        );
        assert_eq!(
            format_download_section("01:30", ""),
            Some("*01:30-inf".into())
        );
        assert_eq!(
            format_download_section("", "03:45"),
            Some("*0-03:45".into())
        );
        assert_eq!(format_download_section("", ""), None);
        assert_eq!(format_download_section("  ", "  "), None);
    }

    #[test]
    fn la_descarga_lleva_download_sections() {
        let options = Options {
            download_sections: Some("*01:30-03:45".into()),
            ..Options::default()
        };
        let (args, _) = download_args("https://ejemplo.test/v", &options);
        assert!(args.iter().any(|arg| arg == "--download-sections"));
        assert!(args.iter().any(|arg| arg == "*01:30-03:45"));
        assert!(args.iter().any(|arg| arg == "--force-keyframes-at-cuts"));
    }

    #[test]
    fn la_descarga_lleva_playlist_item() {
        let options = Options {
            playlist_item: Some(2),
            ..Options::default()
        };
        let (args, _) = download_args("https://ejemplo.test/v", &options);
        assert!(args.iter().any(|arg| arg == "--playlist-items"));
        assert!(args.iter().any(|arg| arg == "2"));
    }
}
