//! La cola de descargas. Vive en su propio hilo para que la interfaz nunca
//! espere a yt-dlp, y habla con la app por canales.
//!
//! El trabajo pesado es yt-dlp, igual que en yoinks: resolver metadatos con
//! `-J`, descargar con `--newline` y leer el progreso linea por linea.

pub mod ytdlp;

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Video,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Format {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    /// Argumentos que se le pasan a yt-dlp para este formato.
    pub args: &'static [&'static str],
}

pub const FORMATS: &[Format] = &[
    Format {
        id: "best",
        label: "Mejor",
        kind: Kind::Video,
        args: &["-f", "bv*+ba/b", "--merge-output-format", "mp4"],
    },
    Format {
        id: "2160p",
        label: "4K",
        kind: Kind::Video,
        args: &[
            "-f",
            "bv*[height<=2160]+ba/b",
            "--merge-output-format",
            "mp4",
        ],
    },
    Format {
        id: "1080p",
        label: "1080p",
        kind: Kind::Video,
        args: &[
            "-f",
            "bv*[height<=1080]+ba/b",
            "--merge-output-format",
            "mp4",
        ],
    },
    Format {
        id: "720p",
        label: "720p",
        kind: Kind::Video,
        args: &[
            "-f",
            "bv*[height<=720]+ba/b",
            "--merge-output-format",
            "mp4",
        ],
    },
    Format {
        id: "mp3",
        label: "mp3",
        kind: Kind::Audio,
        // Metadatos y caratula no van aca: los agrega la opcion "metadatos",
        // que esta encendida por defecto, y repetir el flag es ruido.
        args: &[
            "-f",
            "ba/b",
            "-x",
            "--audio-format",
            "mp3",
            "--audio-quality",
            "0",
        ],
    },
    Format {
        id: "opus",
        label: "opus",
        kind: Kind::Audio,
        args: &["-f", "ba/b", "-x", "--audio-format", "opus"],
    },
];

/// La plantilla de nombre que usa la app cuando no se le dice otra. Vive aca y
/// no en los ajustes para que el worker no dependa de ellos: es el default de
/// yt-dlp con el titulo recortado.
pub const DEFAULT_TEMPLATE: &str = "%(title).120s.%(ext)s";

pub fn format_by_id(id: &str) -> &'static Format {
    FORMATS.iter().find(|f| f.id == id).unwrap_or(&FORMATS[0])
}

/// Opciones que el usuario puede cambiar y que yoinks tiene fijas.
#[derive(Clone, Debug)]
pub struct Options {
    pub format_id: String,
    pub chapters: bool,
    /// `--embed-metadata --embed-thumbnail`: lo que yoinks trae fijo.
    pub metadata: bool,
    pub subtitles: Option<String>,
    /// `--cookies-from-browser`, el issue 11 de yoinks.
    pub cookies_from_browser: Option<String>,
    pub output_dir: Option<std::path::PathBuf>,
    pub filename_template: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            format_id: "best".into(),
            chapters: true,
            metadata: true,
            subtitles: None,
            cookies_from_browser: None,
            output_dir: None,
            filename_template: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Media {
    pub title: String,
    pub uploader: String,
    pub duration: Option<f64>,
    pub host: String,
    pub thumbnail_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Resolviendo metadatos.
    Probing,
    Queued,
    Downloading,
    /// yt-dlp esta uniendo pistas o extrayendo audio: hay trabajo, pero no hay
    /// bytes nuevos. yoinks lo muestra como una pausa sin explicacion.
    Postprocessing,
    Done {
        path: String,
    },
    Failed {
        reason: String,
    },
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub url: String,
    pub media: Media,
    pub options: Options,
    pub state: State,
    /// 0.0 a 1.0.
    pub progress: f32,
    /// Bytes por segundo, tal como los reporta yt-dlp.
    pub speed: Option<f64>,
    pub eta_secs: Option<u64>,
}

impl Job {
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            State::Probing | State::Queued | State::Downloading | State::Postprocessing
        )
    }
}

/// Lo que la interfaz le pide al worker.
#[derive(Debug)]
pub enum Command {
    /// Solo leer el enlace para pintar la ficha. No crea trabajo ni descarga
    /// nada: el usuario todavia tiene que elegir formato.
    Preview {
        url: String,
    },
    Start {
        id: u64,
        url: String,
        options: Options,
    },
    Cancel {
        id: u64,
    },
    Shutdown,
}

/// Lo que el worker le cuenta a la interfaz.
#[derive(Debug)]
pub enum Event {
    Previewed {
        url: String,
        media: Media,
    },
    PreviewFailed {
        reason: String,
    },
    Progress {
        id: u64,
        progress: f32,
        speed: Option<f64>,
        eta_secs: Option<u64>,
    },
    StateChanged {
        id: u64,
        state: State,
    },
}

/// Prueba de la cola sin red ni yt-dlp de verdad: un guion falso que tarda lo
/// que le pidamos, para poder medir si dos trabajos bajan a la vez y si
/// cancelar corta de verdad.
///
/// Corre en un proceso aparte (`reel --download-selfcheck`), asi que puede
/// tocar `REEL_YTDLP` sin pisarle el entorno a nadie.
#[cfg(feature = "selfcheck")]
pub mod selfcheck {
    use super::ytdlp::{download_args, parse_progress, probe, ytdlp_binary, MAX_CONCURRENTES};
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::io::{BufRead, BufReader};
    use std::path::{Path, PathBuf};
    use std::process::{Command as Proc, Stdio};
    use std::time::{Duration, Instant};

    /// Lo que tarda cada trabajo, en segundos. Los tres son distintos para
    /// que se note si van en fila.
    const DURACIONES: [f64; 3] = [1.2, 2.4, 0.8];

    /// Lo que dejo un trabajo al arrancar: con que url, cuanto le tocaba
    /// tardar, y a que hora empezo.
    #[derive(Clone, Debug)]
    struct Arranque {
        url: String,
        segundos: f64,
        cuando: f64,
    }

    /// Un `yt-dlp` de mentira. Escribe las lineas que la app sabe leer y deja
    /// su hora de arranque en un archivo, que es lo unico que permite ver si
    /// dos trabajos se solaparon.
    ///
    /// La duracion la saca de la propia url (`.../v1`, `.../v2`). Es a
    /// proposito: si cada trabajo necesitara su propio binario, habria que
    /// apuntar `REEL_YTDLP` a cada uno y la variable es del proceso entero, o
    /// sea una carrera entre los tres hilos.
    fn escribir_guion(dir: &Path) -> std::io::Result<PathBuf> {
        let path = dir.join("yt-dlp-falso.sh");
        let guion = r#"#!/usr/bin/env bash
# Uso: yt-dlp-falso.sh -P CARPETA -o PLANTILLA URL...
carpeta="."
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -P) carpeta="$2"; shift 2 ;;
    -o|--progress-template|--print|-f|--merge-output-format|--audio-format|--audio-quality|--sub-langs|--cookies-from-browser) shift 2 ;;
    http*) url="$1"; shift ;;
    *) shift ;;
  esac
done

case "$url" in
  *v1) segundos=1.2 ;;
  *v2) segundos=2.4 ;;
  *v3) segundos=0.8 ;;
  *lento) segundos=6.0 ;;
  *) segundos=1.0 ;;
esac

echo "$url|$segundos|$(date +%s.%N)" >> "$carpeta/arranque.txt"
paso=$(awk "BEGIN{print $segundos/4}")
i=1
while [ "$i" -le 4 ]; do
  sleep "$paso"
  echo "PROGRESS| $((i * 25))%|1048576.0|$((4 - i))"
  i=$((i + 1))
done
# El postprocesado se anuncia y se deja un respiro: en la vida real ffmpeg
# tarda, y asi el sondeo alcanza a verlo.
echo "[Merger] Merging formats into $carpeta/reel-prueba-$segundos.mp4"
sleep 0.5
touch "$carpeta/reel-prueba-$segundos.mp4"
echo "DONE|$carpeta/reel-prueba-$segundos.mp4"
"#;
        std::fs::write(&path, guion)?;
        permisos_de_ejecucion(&path)?;
        Ok(path)
    }

    fn permisos_de_ejecucion(path: &Path) -> std::io::Result<()> {
        let mut permisos = std::fs::metadata(path)?.permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permisos, 0o755);
        std::fs::set_permissions(path, permisos)
    }

    /// Lo que dejo cada trabajo al arrancar: su url, la duracion que le toco y
    /// la hora. La duracion importa porque cada trabajo tiene la suya, y asi se
    /// ve si de verdad corrio el que era.
    fn arranques(dir: &Path) -> Vec<Arranque> {
        let Ok(text) = std::fs::read_to_string(dir.join("arranque.txt")) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| {
                let mut partes = line.trim().split('|');
                Some(Arranque {
                    url: partes.next()?.to_string(),
                    segundos: partes.next()?.parse().ok()?,
                    cuando: partes.next()?.parse().ok()?,
                })
            })
            .collect()
    }

    /// Encola un trabajo igual que la app: la fila primero (`push_ready`), y
    /// despues la orden. Al reves no hay a quien aplicarle los eventos, que es
    /// justo lo que hace que una fila se quede en "leyendo" para siempre.
    fn encolar(backend: &Backend, id: u64, url: &str, salida: &Path) {
        let options = Options {
            output_dir: Some(salida.to_path_buf()),
            ..Options::default()
        };
        {
            let mut queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.push_ready(url.to_string(), options.clone(), Media::default());
        }
        backend.send(Command::Start {
            id,
            url: url.to_string(),
            options,
        });
    }

    /// Cuantos trabajos estuvieron corriendo a la vez, como maximo, mirando
    /// solo las horas de arranque. Devuelve el maximo y los intervalos.
    fn maximo_solapados(intervalos: &[(f64, f64)]) -> usize {
        let mut maximo = 0;
        for (arranque, _) in intervalos {
            let vivos = intervalos
                .iter()
                .filter(|(otro_arranque, otro_fin)| {
                    otro_arranque <= arranque && arranque < otro_fin
                })
                .count();
            maximo = maximo.max(vivos);
        }
        maximo
    }

    /// La duracion con la que el guion contesta a cada url.
    fn duracion_de(id: u64) -> f64 {
        DURACIONES[(id as usize - 1) % DURACIONES.len()]
    }

    /// Encola `n` trabajos y espera a que todos terminen. Devuelve cuanto tardo
    /// todo, lo que dejo cada arranque y el estado final de cada trabajo.
    fn correr(n: usize, salida: &Path) -> (Duration, Vec<Arranque>, HashMap<u64, State>) {
        let guion = escribir_guion(salida).expect("deberia escribir el yt-dlp falso");
        // Un solo binario para todos: la variable es del proceso, asi que
        // apuntarla a uno distinto por trabajo seria una carrera.
        std::env::set_var("REEL_YTDLP", &guion);
        let backend = Backend::spawn(|| {});

        let arranque = Instant::now();
        for id in 1..=n as u64 {
            encolar(&backend, id, &format!("https://ejemplo.test/v{id}"), salida);
        }

        let limite = Instant::now() + Duration::from_secs(30);
        let mut estados: HashMap<u64, State> = HashMap::new();
        while estados.len() < n && Instant::now() < limite {
            backend.drain();
            {
                let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
                for job in &queue.jobs {
                    if !job.is_active() {
                        estados.insert(job.id, job.state.clone());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        let total = arranque.elapsed();

        backend.send(Command::Shutdown);
        (total, arranques(salida), estados)
    }

    /// Dos trabajos a la vez tienen que solaparse. Si la cola volviera a ser
    /// secuencial, el segundo arrancaria recien cuando termina el primero.
    fn comprobar_concurrencia(salida: &Path) {
        let (total, marcas, estados) = correr(3, salida);
        let suma: f64 = DURACIONES.iter().sum();

        // Ordenadas por trabajo, que es como las escribio el guion.
        let mut por_trabajo = marcas.clone();
        por_trabajo.sort_by(|a, b| a.url.cmp(&b.url));
        let duraciones: Vec<f64> = por_trabajo.iter().map(|marca| marca.segundos).collect();
        let mut tiempos: Vec<f64> = marcas.iter().map(|marca| marca.cuando).collect();
        tiempos.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        println!("total      {total:?}  (en serie serian ~{suma}s)");
        println!("arranques  {tiempos:?}");
        println!("duraciones {duraciones:?}");
        println!("estados    {estados:?}");

        assert_eq!(
            estados.len(),
            3,
            "faltan trabajos por terminar: {estados:?}"
        );
        for (id, estado) in &estados {
            assert!(
                matches!(estado, State::Done { .. }),
                "el trabajo {id} termino en {estado:?}"
            );
        }
        assert!(
            total < Duration::from_secs_f64(DURACIONES[1] + 1.5),
            "tres trabajos tardaron {total:?}: en serie serian ~{suma}s, o sea que no son concurrentes"
        );
        assert_eq!(marcas.len(), 3, "no arrancaron los tres: {marcas:?}");
        // Cada trabajo tiene que haber corrido con SU duracion: si todos
        // arrancaron con la misma, la ruta del binario se leyo tarde.
        let esperadas: Vec<f64> = (1..=3).map(duracion_de).collect();
        assert_eq!(
            duraciones, esperadas,
            "no corrio la duracion que le tocaba a cada trabajo"
        );
        assert!(
            tiempos[1] - tiempos[0] < DURACIONES[1] / 2.0,
            "los dos primeros arranques distan {}s: no se solaparon",
            tiempos[1] - tiempos[0]
        );
        println!("OK concurrencia");
    }

    /// A mitad de camino el trabajo tiene que estar "descargando" con
    /// progreso, y pasar por el postprocesado. La fila no puede mentir.
    fn comprobar_estado(salida: &Path) {
        let guion = escribir_guion(salida).expect("deberia escribir el yt-dlp falso");
        std::env::set_var("REEL_YTDLP", &guion);

        let backend = Backend::spawn(|| {});
        encolar(&backend, 1, "https://ejemplo.test/v1", salida);

        let limite = Instant::now() + Duration::from_secs(15);
        let mut vio_descarga = false;
        let mut vio_progreso = false;
        let mut vio_postproceso = false;
        let mut vio_en_espera = false;
        let mut finales: Option<State> = None;

        while Instant::now() < limite {
            backend.drain();
            {
                let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(job) = queue.jobs.first() {
                    match &job.state {
                        State::Queued => vio_en_espera = true,
                        State::Downloading => vio_descarga = true,
                        State::Postprocessing => vio_postproceso = true,
                        State::Done { .. } | State::Failed { .. } | State::Cancelled => {
                            finales = Some(job.state.clone());
                        }
                        State::Probing => {}
                    }
                    if job.progress > 0.0 {
                        vio_progreso = true;
                    }
                }
            }
            if finales.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        backend.send(Command::Shutdown);
        println!(
            "espera={vio_en_espera} descarga={vio_descarga} progreso={vio_progreso} postproceso={vio_postproceso} final={finales:?}"
        );
        assert!(vio_descarga, "nunca se vio el trabajo descargando");
        assert!(vio_progreso, "nunca llego progreso");
        assert!(vio_postproceso, "nunca se vio el postprocesado de ffmpeg");
        assert!(
            matches!(finales, Some(State::Done { .. })),
            "no termino listo: {finales:?}"
        );
        println!("OK estado");
    }

    /// Cancelar tiene que matar el hijo y dejar el trabajo cancelado, no
    /// "listo" ni "fallo".
    fn comprobar_cancelacion(salida: &Path) {
        let guion = escribir_guion(salida).expect("deberia escribir el yt-dlp falso");
        // La url "lento" hace que el guion tarde 6s: tiempo de sobra para
        // cortarlo y notar si de verdad lo mato.
        std::env::set_var("REEL_YTDLP", &guion);

        let backend = Backend::spawn(|| {});
        encolar(&backend, 1, "https://ejemplo.test/lento", salida);

        // Se espera a que este descargando de verdad antes de cortarlo.
        let limite = Instant::now() + Duration::from_secs(10);
        let mut descargando = false;
        while Instant::now() < limite && !descargando {
            backend.drain();
            let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
            descargando = queue
                .jobs
                .first()
                .is_some_and(|job| matches!(job.state, State::Downloading));
        }
        assert!(
            descargando,
            "nunca llego a descargar: no hay nada que cancelar"
        );

        let arranque = Instant::now();
        backend.send(Command::Cancel { id: 1 });

        let limite = Instant::now() + Duration::from_secs(10);
        let mut estado: Option<State> = None;
        while Instant::now() < limite {
            backend.drain();
            {
                let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(job) = queue.jobs.first() {
                    if !job.is_active() {
                        estado = Some(job.state.clone());
                    }
                }
            }
            if estado.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let tardo = arranque.elapsed();
        backend.send(Command::Shutdown);

        println!("cancelado en {tardo:?} -> {estado:?}");
        assert!(
            matches!(estado, Some(State::Cancelled)),
            "quedo en {estado:?} en vez de cancelado"
        );
        // El guion tarda 6s: tardar mas que eso significa que no lo mato.
        assert!(
            tardo < Duration::from_secs(3),
            "tardo {tardo:?} en cancelar: el hijo siguio vivo"
        );
        println!("OK cancelacion");
    }

    /// Encolar mas trabajos que el cupo no puede lanzar mas yt-dlp de los
    /// permitidos: los que sobran esperan su lugar.
    fn comprobar_limite(salida: &Path) {
        const TRABAJOS: u64 = 6;
        let guion = escribir_guion(salida).expect("deberia escribir el yt-dlp falso");
        // Todas las urls duran 1s: sin tope arrancarian las seis a la vez.
        std::env::set_var("REEL_YTDLP", &guion);
        let backend = Backend::spawn(|| {});

        for id in 1..=TRABAJOS {
            encolar(&backend, id, &format!("https://ejemplo.test/v{id}"), salida);
        }

        let limite = Instant::now() + Duration::from_secs(90);
        let mut estados: HashMap<u64, State> = HashMap::new();
        while estados.len() < TRABAJOS as usize && Instant::now() < limite {
            backend.drain();
            {
                let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
                for job in &queue.jobs {
                    if !job.is_active() {
                        estados.insert(job.id, job.state.clone());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        backend.send(Command::Shutdown);

        let marcas = arranques(salida);
        assert_eq!(
            marcas.len(),
            TRABAJOS as usize,
            "no arrancaron todos: {marcas:?}"
        );
        assert_eq!(
            estados.len(),
            TRABAJOS as usize,
            "faltan finales: {estados:?}"
        );
        for (id, estado) in &estados {
            assert!(
                matches!(estado, State::Done { .. }),
                "el trabajo {id} termino en {estado:?}"
            );
        }

        let intervalos: Vec<(f64, f64)> = marcas
            .iter()
            .map(|marca| (marca.cuando, marca.cuando + marca.segundos))
            .collect();
        let solapados = maximo_solapados(&intervalos);
        println!("corrieron a la vez, como maximo: {solapados} (cupo {MAX_CONCURRENTES})");
        assert!(
            solapados <= MAX_CONCURRENTES,
            "corrieron {solapados} a la vez con un cupo de {MAX_CONCURRENTES}"
        );
        assert!(
            solapados >= 2,
            "nunca corrieron dos a la vez: no hay concurrencia"
        );
        println!("OK limite");
    }

    /// Cancelar justo cuando el trabajo esta terminando: el estado no puede
    /// pasar de "cancelado" a "listo". Es la carrera entre el hilo del trabajo
    /// y el supervisor, asi que se estresa varias veces.
    fn comprobar_carrera_entre_terminar_y_cancelar(salida: &Path) {
        const VUELTAS: u64 = 12;
        const TIROS: u64 = 4;

        let guion = escribir_guion(salida).expect("deberia escribir el yt-dlp falso");
        std::env::set_var("REEL_YTDLP", &guion);
        let backend = Backend::spawn(|| {});

        let mut cancelados = 0;
        let mut tardios = 0;
        let mut nunca_terminaron = 0;
        let mut siguiente = 1;

        for vuelta in 0..VUELTAS {
            let ids: Vec<u64> = (0..TIROS)
                .map(|_| {
                    let id = siguiente;
                    siguiente += 1;
                    id
                })
                .collect();
            for id in &ids {
                let url = format!("https://ejemplo.test/v{}", id % 3 + 1);
                encolar(&backend, *id, &url, salida);
            }
            // Un rato despues del arranque, para pegarle al final.
            std::thread::sleep(Duration::from_millis(700));
            for id in &ids {
                backend.send(Command::Cancel { id: *id });
            }

            // Se mira cada trabajo hasta que se quede quieto, y se anota si
            // despues de cancelado igual conto un final.
            let limite = Instant::now() + Duration::from_secs(20);
            let mut vistos: HashSet<u64> = HashSet::new();
            let mut esperando: HashSet<u64> = ids.iter().copied().collect();
            let mut ya_cancelado: HashSet<u64> = HashSet::new();

            while !esperando.is_empty() && Instant::now() < limite {
                backend.drain();
                let jobs = {
                    let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
                    queue.jobs.clone()
                };
                for job in &jobs {
                    if !esperando.contains(&job.id) {
                        continue;
                    }
                    match &job.state {
                        State::Cancelled => {
                            ya_cancelado.insert(job.id);
                            vistos.insert(job.id);
                        }
                        State::Done { .. } | State::Failed { .. } => {
                            if ya_cancelado.contains(&job.id) {
                                tardios += 1;
                                println!(
                                    "el trabajo {} conto {:?} despues de cancelado",
                                    job.id, job.state
                                );
                            }
                            vistos.insert(job.id);
                        }
                        _ => {}
                    }
                }
                // Cancelado o terminado, ya no hay mas que mirarle.
                esperando.retain(|id| !vistos.contains(id));
                std::thread::sleep(Duration::from_millis(20));
            }
            nunca_terminaron += esperando.len();

            for id in &ids {
                let estado = {
                    let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
                    queue
                        .jobs
                        .iter()
                        .find(|job| job.id == *id)
                        .map(|job| job.state.clone())
                };
                if matches!(estado, Some(State::Cancelled)) {
                    cancelados += 1;
                }
            }
            println!("vuelta {vuelta}: ids {ids:?}");
        }

        backend.send(Command::Shutdown);
        println!("cancelados={cancelados} tardios={tardios} sin_terminar={nunca_terminaron}");
        assert_eq!(tardios, 0, "hubo {tardios} finales despues de cancelar");
        assert_eq!(
            nunca_terminaron, 0,
            "quedaron {nunca_terminaron} sin terminar"
        );
        assert!(
            cancelados >= 4,
            "casi ninguno llego a cancelarse ({cancelados} de {VUELTAS}x{TIROS}): la prueba no probo nada"
        );
        println!("OK carrera");
    }

    /// Un video libre, como los que la app va a bajar de verdad.
    const VIDEO_DE_PRUEBA: &str = "https://archive.org/details/BigBuckBunny_124";

    /// La unica prueba que sale a la red, y por eso no corre con las demas.
    ///
    /// Las demas usan un yt-dlp de mentira, asi que si alguien cambia un flag
    /// por uno que yt-dlp no conoce, ninguna se entera. Esta comprueba el
    /// contrato contra el yt-dlp de verdad: que lea los metadatos como la app
    /// espera, que acepte el selector de formato, y que el progreso llegue con
    /// la forma que sabemos parsear.
    ///
    /// Mirar el progreso necesita que algo se baje, asi que se baja el archivo
    /// mas chico que ofrezca el sitio y se corta ahi: verifica la tuberia sin
    /// traer el video entero.
    fn comprobar_descarga_real(salida: &Path) {
        // Se usa el yt-dlp del sistema, no el de mentira.
        std::env::remove_var("REEL_YTDLP");

        let media = match probe(VIDEO_DE_PRUEBA) {
            Ok(media) => {
                println!(
                    "probe OK: {:?} de {:?} ({}s) en {}",
                    media.title,
                    media.uploader,
                    media.duration.unwrap_or(0.0),
                    media.host
                );
                media
            }
            Err(reason) => {
                eprintln!("no pude leer el enlace de prueba: {reason}");
                eprintln!("esto necesita internet y yt-dlp en el PATH");
                panic!("probe fallo");
            }
        };
        assert!(!media.title.is_empty(), "el probe no trajo titulo");
        assert!(!media.host.is_empty(), "el probe no trajo de donde viene");

        // Los mismos argumentos que arma la app, en seco: si un flag no existe
        // o el selector de formato es invalido, yt-dlp lo dice aca.
        let options = Options {
            output_dir: Some(salida.to_path_buf()),
            ..Options::default()
        };
        let (args, _) = download_args(VIDEO_DE_PRUEBA, &options);

        let mut en_seco = args.clone();
        en_seco.push("--skip-download".into());
        let seco = Proc::new(ytdlp_binary())
            .args(&en_seco)
            .output()
            .expect("deberia lanzar yt-dlp");
        if !seco.status.success() {
            eprintln!("{}", String::from_utf8_lossy(&seco.stderr));
        }
        assert!(
            seco.status.success(),
            "yt-dlp no acepto los argumentos de la app: {:?}",
            seco.status
        );
        println!("argumentos OK (con --skip-download)");

        // Y ahora si, la tuberia de progreso: el archivo mas chico del sitio,
        // que se corta apenas llegan lineas con la forma esperada.
        let mut con_progreso = args.clone();
        con_progreso.push("-f".into());
        con_progreso.push("worst".into());
        let mut hijo = Proc::new(ytdlp_binary())
            .args(&con_progreso)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("deberia lanzar yt-dlp");

        let mut progresos = 0;
        {
            let stdout = hijo.stdout.take().expect("deberia tener stdout");
            let lector = BufReader::new(stdout);
            for line in lector.lines().map_while(Result::ok) {
                if let Some(rest) = line.strip_prefix("PROGRESS|") {
                    if parse_progress(1, rest).is_some() {
                        progresos += 1;
                    }
                }
                // Con un par de lineas bien formadas ya se probo lo que hacia
                // falta: no hay que esperar a que baje todo.
                if progresos >= 2 {
                    let _ = hijo.kill();
                    break;
                }
            }
        }
        let _ = hijo.wait();

        assert!(
            progresos > 0,
            "yt-dlp no imprimio ninguna linea que supieramos leer: la plantilla no coincide"
        );
        println!("OK descarga-real ({progresos} lineas de progreso leidas)");
    }

    /// Corre en un proceso propio: prepara un directorio, elige la prueba y
    /// devuelve el codigo de salida. Cero significa que paso.
    pub fn run(modo: &str) -> i32 {
        let salida = std::env::temp_dir().join(format!("reel-selfcheck-{}", std::process::id()));
        if let Err(error) = std::fs::create_dir_all(&salida) {
            eprintln!("no pude preparar {}: {error}", salida.display());
            return 2;
        }

        let resultado = std::panic::catch_unwind(|| match modo {
            "concurrencia" => comprobar_concurrencia(&salida),
            "estado" => comprobar_estado(&salida),
            "cancelacion" => comprobar_cancelacion(&salida),
            "limite" => comprobar_limite(&salida),
            "carrera" => comprobar_carrera_entre_terminar_y_cancelar(&salida),
            "descarga-real" => comprobar_descarga_real(&salida),
            otro => panic!("prueba desconocida: {otro}"),
        });

        let _ = std::fs::remove_dir_all(&salida);
        if resultado.is_ok() {
            0
        } else {
            eprintln!("la prueba {modo} fallo");
            1
        }
    }
}

/// La cola completa, compartida con la interfaz.
#[derive(Default)]
pub struct Queue {
    pub jobs: Vec<Job>,
    /// Lo ultimo que se leyo de un enlace, para la ficha de arriba.
    pub preview: Option<(String, Media)>,
    pub preview_error: Option<String>,
    next_id: u64,
}

impl Queue {
    pub fn push(&mut self, url: String, options: Options) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.jobs.push(Job {
            id,
            url,
            media: Media::default(),
            options,
            state: State::Probing,
            progress: 0.0,
            speed: None,
            eta_secs: None,
        });
        id
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    pub fn active(&self) -> usize {
        self.jobs.iter().filter(|j| j.is_active()).count()
    }

    pub fn done(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| matches!(j.state, State::Done { .. }))
            .count()
    }

    /// Encola un trabajo que ya tiene sus metadatos leidos: la ficha ya los
    /// trajo, asi que la fila nace con titulo y en espera de arrancar.
    pub fn push_ready(&mut self, url: String, options: Options, media: Media) -> u64 {
        let id = self.push(url, options);
        if let Some(job) = self.get_mut(id) {
            job.media = media;
            job.state = State::Queued;
        }
        id
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Previewed { url, media } => {
                self.preview_error = None;
                self.preview = Some((url, media));
            }
            Event::PreviewFailed { reason } => {
                self.preview = None;
                self.preview_error = Some(reason);
            }
            Event::Progress {
                id,
                progress,
                speed,
                eta_secs,
            } => {
                if let Some(job) = self.get_mut(id) {
                    job.progress = progress;
                    job.speed = speed;
                    job.eta_secs = eta_secs;
                }
            }
            Event::StateChanged { id, state } => {
                if let Some(job) = self.get_mut(id) {
                    if matches!(state, State::Done { .. }) {
                        job.progress = 1.0;
                    }
                    job.state = state;
                }
            }
        }
    }
}

/// Handle del worker: la app guarda esto y nada mas.
pub struct Backend {
    commands: Sender<Command>,
    events: Receiver<Event>,
    pub queue: Arc<Mutex<Queue>>,
}

impl Backend {
    pub fn spawn<W>(wake: W) -> Self
    where
        W: Fn() + Send + Sync + 'static,
    {
        let (commands, command_rx) = std::sync::mpsc::channel::<Command>();
        let (event_tx, events) = std::sync::mpsc::channel::<Event>();
        let queue = Arc::new(Mutex::new(Queue::default()));

        std::thread::Builder::new()
            .name("reel-worker".into())
            .spawn(move || ytdlp::worker(command_rx, event_tx, wake))
            .expect("no se pudo crear el hilo de descargas");

        Self {
            commands,
            events,
            queue,
        }
    }

    pub fn send(&self, command: Command) {
        if let Err(error) = self.commands.send(command) {
            log::warn!("el hilo de descargas no acepta mas ordenes: {error}");
        }
    }

    /// Vacia el canal de eventos sobre la cola. Devuelve true si algo cambio,
    /// para que el frame solo se repinte cuando hace falta.
    pub fn drain(&self) -> bool {
        let mut changed = false;
        let mut queue = match self.queue.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        for event in self.events.try_iter() {
            queue.apply(event);
            changed = true;
        }
        changed
    }
}
