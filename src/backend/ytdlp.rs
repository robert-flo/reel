//! Los hilos que hablan con yt-dlp.
//!
//! Un trabajo, un hilo: la cola baja varios a la vez, que es lo que promete el
//! README y lo que el boceto muestra. El hilo supervisor solo despacha ordenes
//! y nunca espera a un trabajo, asi que cancelar y encolar siguen andando
//! mientras abajo se descarga.
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
use std::time::Duration;

use super::{format_by_id, Command, Event, Kind, Media, Options, State};

/// Plantilla de progreso: una linea por actualizacion, campos separados por
/// `|`, sin nada que parsear a ojo.
const PROGRESS_TEMPLATE: &str =
    "download:PROGRESS|%(progress._percent_str)s|%(progress.speed)s|%(progress.eta)s";

/// yt-dlp del PATH. Si algun dia se quiere el binario propio (como yoinks, que
/// lo baja a ~/.yoinks/bin), este es el unico lugar que cambia. `REEL_YTDLP`
/// apunta a otro, que es como se prueban las carreras sin bajar nada.
pub(crate) fn ytdlp_binary() -> PathBuf {
    std::env::var_os("REEL_YTDLP")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("yt-dlp"))
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

/// Cuantos trabajos pueden bajar a la vez. Sin tope, encolar treinta enlaces
/// lanzaria treinta yt-dlp y treinta ffmpeg: la maquina deja de responder y las
/// descargas se estorban entre si. Los que sobran quedan en espera hasta que
/// se libere un cupo.
pub const MAX_CONCURRENTES: usize = 3;

/// El cupo de descargas simultaneas. Se pide antes de arrancar un trabajo y se
/// devuelve cuando termina; pedirlo no bloquea al supervisor mas que un rato
/// corto, porque los cupos se liberan solos.
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

    /// Espera a que haya lugar. Mira cada pocos milisegundos en vez de dormir
    /// sobre una condicion: los cupos se liberan desde otros hilos y el
    /// supervisor tiene que poder despertar a atender lo que llegue.
    fn tomar(&self, limite: Duration) -> Option<Cupo<'_>> {
        let espera = Duration::from_millis(5);
        let arranque = std::time::Instant::now();
        loop {
            {
                let mut libres = lock(&self.libres);
                if *libres > 0 {
                    *libres -= 1;
                    return Some(Cupo { cupos: self });
                }
            }
            if arranque.elapsed() > limite {
                return None;
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

    while let Ok(command) = commands.recv() {
        match command {
            Command::Shutdown => break,
            Command::Cancel { id } => {
                cancel(&running, &cancelled, &events, &*wake, id);
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
                // Se resuelve aca y no dentro del hilo: con varios trabajos a
                // la vez, leerlo alla seria una carrera entre todos.
                let ytdlp = ytdlp_binary();

                let handle = std::thread::Builder::new()
                    .name(format!("reel-job-{id}"))
                    .spawn(move || {
                        // Esperar cupo es cosa del trabajo, no del supervisor:
                        // asi el resto de la cola sigue andando mientras tanto.
                        if let Some(_cupo) = cupos.tomar(Duration::from_secs(600)) {
                            if let Err(error) = run_job(
                                id,
                                &ytdlp,
                                &url,
                                &options,
                                &running,
                                &cancelled,
                                &events_job,
                                &*wake_job,
                                &alive,
                            ) {
                                log::error!("no pude atender el trabajo {id}: {error}");
                            }
                        } else {
                            let _ = events_job.send(Event::StateChanged {
                                id,
                                state: State::Failed {
                                    reason: "espere demasiado por un lugar en la cola".into(),
                                },
                            });
                            wake_job();
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
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    Ok(Media {
        title: json["title"].as_str().unwrap_or("Sin titulo").to_string(),
        uploader: json["uploader"]
            .as_str()
            .or_else(|| json["channel"].as_str())
            .unwrap_or("")
            .to_string(),
        duration: json["duration"].as_f64(),
        host: json["extractor_key"].as_str().unwrap_or("").to_lowercase(),
        thumbnail_url: json["thumbnail"].as_str().map(|s| s.to_string()),
    })
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
        "--progress-template".into(),
        PROGRESS_TEMPLATE.into(),
        "--no-playlist".into(),
        "-P".into(),
        dir.display().to_string(),
        "-o".into(),
        template,
        "--print".into(),
        "after_move:DONE|%(filepath)s".into(),
    ];

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
        args.push("--sub-langs".into());
        args.push(languages.clone());
        args.push("--embed-subs".into());
    }
    if let Some(browser) = &options.cookies_from_browser {
        args.push("--cookies-from-browser".into());
        args.push(browser.clone());
    }
    args.push(url.into());

    (args, dir)
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
    wake: &W,
    alive: &AtomicBool,
) -> std::io::Result<()>
where
    W: Fn(),
{
    let (args, dir) = download_args(url, options);

    let mut child = Proc::new(ytdlp)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    lock(running).insert(id, child);
    // Ya hay un yt-dlp corriendo para este trabajo: ahora si.
    let _ = events.send(Event::StateChanged {
        id,
        state: State::Downloading,
    });
    wake();

    // Lo que yt-dlp escriba en stderr se junta en su propio hilo. Si nadie lo
    // leyera, el buffer de la tuberia se llena y yt-dlp se queda esperando:
    // las dos partes trabadas.
    let collected = Arc::new(Mutex::new(String::new()));
    let reader = {
        let collected = Arc::clone(&collected);
        stderr.map(|mut stderr| {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = stderr.read_to_string(&mut text);
                *lock(&collected) = text;
            })
        })
    };

    let mut final_path: Option<String> = None;
    if let Some(stdout) = stdout {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(rest) = line.strip_prefix("PROGRESS|") {
                if let Some(event) = parse_progress(id, rest) {
                    let _ = events.send(event);
                    wake();
                }
            } else if let Some(path) = line.strip_prefix("DONE|") {
                final_path = Some(path.trim().to_string());
            } else if line.contains("[Merger]") || line.contains("[ExtractAudio]") {
                let _ = events.send(Event::StateChanged {
                    id,
                    state: State::Postprocessing,
                });
                wake();
            }
        }
    }

    // El candado se suelta antes de esperar: si esperara con el puesto, una
    // cancelacion no podria ni entrar a matar el hijo.
    let child = lock(running).remove(&id);
    let status = child.map(|mut child| child.wait());
    if let Some(reader) = reader {
        let _ = reader.join();
    }

    // La marca de cancelado es la fuente de verdad, y `cancel` la pone antes de
    // matar el hijo. Asi, cuando este hilo mira, ya esta: nunca reporta un
    // "listo" encima de un "cancelado". Durante el cierre, ademas, no hay a
    // quien contarle nada.
    if lock(cancelled).contains(&id) || !alive.load(Ordering::SeqCst) {
        return Ok(());
    }

    let state = match status {
        Some(Ok(status)) if status.success() => State::Done {
            path: final_path.unwrap_or_else(|| dir.display().to_string()),
        },
        Some(Ok(_)) => State::Failed {
            reason: first_useful_line(&lock(&collected)),
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

    let _ = events.send(Event::StateChanged { id, state });
    wake();
    Ok(())
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
                let cupo = cupos
                    .tomar(Duration::from_secs(10))
                    .expect("deberia haber lugar");
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

    /// Sin lugar en el tiempo pedido, se rinde en vez de esperar para siempre.
    #[test]
    fn el_cupo_se_rinde_si_espera_demasiado() {
        let cupos = Cupos::new(1);
        let _guardado = cupos.tomar(Duration::from_millis(50)).expect("el primero");
        assert!(cupos.tomar(Duration::from_millis(50)).is_none());
        drop(_guardado);
        assert!(cupos.tomar(Duration::from_millis(50)).is_some());
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
}
