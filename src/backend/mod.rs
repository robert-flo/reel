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
    /// El enlace es una lista: en vez de bajarla entera como un trabajo, se
    /// expande a una fila por video. La fila de la lista queda como resumen.
    pub playlist: bool,
    /// Volver a bajar aunque el archivo ya este en la carpeta.
    ///
    /// yt-dlp saltea un archivo que existe, tambien al reintentar, asi que un
    /// archivo truncado —un postprocesado que fallo a medias— no se arregla
    /// reintentando. Esto es la salida para ese caso, y por eso es una accion
    /// aparte y no lo que hace `reintentar`: bajar de cero algo que ya esta
    /// bien seria tirar ancho de banda.
    pub force: bool,
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
            playlist: false,
            force: false,
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
    /// Cuando el enlace es una playlist y no un video: cuantos trae. `None`
    /// para un video suelto. Sirve para avisar antes de encolar, porque
    /// `--no-playlist` no frena una url de playlist: la baja entera.
    pub playlist_count: Option<u64>,
    /// La url de este video. Vacia cuando es la misma que la del trabajo (un
    /// video suelto); con algo, cuando viene de una lista y es otra.
    pub url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Resolviendo metadatos.
    Probing,
    Queued,
    Downloading,
    /// yt-dlp esta uniendo pistas o extrayendo audio: hay trabajo, pero no hay
    /// bytes nuevos. yoinks lo muestra como una pausa sin explicacion. El paso
    /// viene del `postprocess:` de yt-dlp, no de adivinar sus textos.
    Postprocessing {
        postprocessor: String,
    },
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
    /// El paso de postprocesado en curso, si hay uno. Se recuerda aparte del
    /// estado para poder contarlo en la fila y no perderlo al cambiar.
    pub postprocessor: Option<String>,
}

impl Job {
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            State::Probing | State::Queued | State::Downloading | State::Postprocessing { .. }
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
    /// Expande una lista a una fila por video. Se pide despues de encolarla,
    /// asi que la fila de la lista ya existe y se le puede aplicar el
    /// resultado.
    Expandir {
        id: u64,
        url: String,
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
    /// Los videos que traia una lista, ya en filas. La lista se encola como
    /// resumen y cada entrada es un trabajo propio, con su progreso.
    PlaylistExpandida {
        id: u64,
        videos: Vec<Media>,
        error: Option<String>,
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
#[cfg(test)]
mod queue_tests {
    use super::*;

    fn job_terminado(queue: &mut Queue, state: State) -> u64 {
        let id = queue.push_ready(
            "https://ejemplo.test/v".into(),
            Options::default(),
            Media::default(),
        );
        let job = queue.get_mut(id).expect("deberia estar");
        job.state = state;
        job.progress = 1.0;
        job.postprocessor = Some("Merger".into());
        id
    }

    #[test]
    fn reintentar_devuelve_los_mismos_datos() {
        let mut queue = Queue::default();
        let id = job_terminado(&mut queue, State::Cancelled);

        let (url, options) = queue.retry(id).expect("deberia poder reintentar");
        assert_eq!(url, "https://ejemplo.test/v");
        assert_eq!(options.format_id, "best");

        let job = queue.get_mut(id).expect("deberia seguir");
        assert_eq!(job.state, State::Queued);
        assert_eq!(job.progress, 0.0);
        assert!(job.speed.is_none());
        assert!(job.eta_secs.is_none());
        assert!(job.postprocessor.is_none());
        // La url, el formato y la carpeta son los mismos: eso es lo que hace
        // que yt-dlp reanude el `.part` en vez de empezar de cero.
        assert_eq!(job.url, url);
    }

    /// Volver a bajar es a proposito y no se arrastra: despues de pedirlo una
    /// vez, un reintento normal ya no fuerza nada. Si no, cada reintento
    /// bajaria de cero algo que ya esta.
    #[test]
    fn volver_a_bajar_no_se_arrastra() {
        let mut queue = Queue::default();
        let id = job_terminado(
            &mut queue,
            State::Done {
                path: "/tmp/roto.mp4".into(),
            },
        );

        let (_, options) = queue.retry_forzado(id).expect("deberia poder");
        assert!(options.force, "la primera vez baja de cero");

        // Termino el intento forzado, como pasaria de verdad.
        queue.get_mut(id).expect("sigue").state = State::Done {
            path: "/tmp/roto.mp4".into(),
        };

        // El segundo intento vuelve a ser normal: la bandera no quedo pegada.
        let (_, options) = queue.retry(id).expect("deberia poder");
        assert!(!options.force, "el reintento normal no baja de cero");
    }

    #[test]
    fn no_se_reintenta_lo_que_esta_andando() {
        let mut queue = Queue::default();
        let id = job_terminado(&mut queue, State::Downloading);
        assert!(
            queue.retry(id).is_none(),
            "no deberia reintentar algo activo"
        );
        assert!(queue.retry(999).is_none(), "un id que no existe tampoco");
    }
}

#[cfg(feature = "selfcheck")]
pub mod selfcheck {
    use super::ytdlp::{
        download_args, leer_linea, lock, probe, ytdlp_binary, Salida, MAX_CONCURRENTES,
    };
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::io::{BufRead, BufReader, Read};
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

    /// Un `yt-dlp` de mentira. Escribe las lineas que la app sabe leer por donde
    /// las escribe yt-dlp de verdad: el progreso y los avisos de postprocesado
    /// por stderr, y el `DONE` de `--print` por stdout. Deja ademas su hora de
    /// arranque en un archivo, que es lo unico que permite ver si dos trabajos
    /// se solaparon.
    ///
    /// La duracion la saca de la propia url (`.../v1`, `.../v2`). Es a
    /// proposito: si cada trabajo necesitara su propio binario, habria que
    /// apuntar `REEL_YTDLP` a cada uno y la variable es del proceso entero, o
    /// sea una carrera entre los tres hilos.
    fn escribir_guion(dir: &Path) -> std::io::Result<PathBuf> {
        let path = dir.join("yt-dlp-falso.sh");
        let guion = r#"#!/usr/bin/env bash
# Uso: yt-dlp-falso.sh -P CARPETA -o PLANTILLA URL...
# Los argumentos se guardan antes del bucle: el `while` hace `shift` y los
# consume, asi que despues `$*` queda vacio y ningun `case` matchearia.
argumentos="$*"
carpeta="."
url=""
fallar=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --fail) fallar=1; shift ;;
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

# `--flat-playlist` pide solo el listado, no una descarga: se contesta con dos
# videos de mentira, con urls que el mismo guion sabe atender.
case " $argumentos " in
  *" --flat-playlist "*)
    cat <<JSON
{"_type":"playlist","title":"Una lista","playlist_count":2,"entries":[
 {"title":"Primer video","url":"https://ejemplo.test/v1","duration":120,
  "uploader":"alguien","ie_key":"Youtube",
  "thumbnails":[{"url":"https://ejemplo.test/uno.jpg"}]},
 {"title":"Segundo video","url":"https://ejemplo.test/v2","duration":240,
  "uploader":"otro","ie_key":"Youtube",
  "thumbnails":[{"url":"https://ejemplo.test/dos.jpg"}]}]}
JSON
    exit 0
    ;;
esac

echo "$url|$segundos|$(date +%s.%N)" >> "$carpeta/arranque.txt"
final="$carpeta/reel-prueba-$segundos.mp4"
parte="$final.part"

# Cuantas veces se intento este archivo. Un reintento pasa a la segunda.
intentos=$(cat "$parte.intentos" 2>/dev/null || echo 0)
intentos=$((intentos + 1))
echo "$intentos" > "$parte.intentos"
echo "intento=$intentos $url" >> "$carpeta/reanudaciones.txt"

# `--fail` hace fallar el intento a proposito, para poder probar el reintento.
if [ "$fallar" = "1" ] && [ "$intentos" -eq 1 ]; then
  echo "PROGRESS| 50.0%|1048576.0|9" >&2
  echo "ERROR: fallo a proposito en el intento 1" >&2
  exit 1
fi

paso=$(awk "BEGIN{print $segundos/4}")
i=1
while [ "$i" -le 4 ]; do
  sleep "$paso"
  # Por stderr: es por donde yt-dlp manda el progreso, y leerlo de stdout era
  # justo el bug que dejo la deteccion del postprocesado sin funcionar.
  echo "PROGRESS| $((i * 25))%|1048576.0|$((4 - i))" >&2
  # Lo bajado va al `.part`, que es lo que yt-dlp reanuda.
  : >> "$parte"
  i=$((i + 1))
done
# El postprocesado se anuncia igual que yt-dlp: con su progress-template, por
# stderr, y con un respiro para que el sondeo lo alcance a ver, como con ffmpeg.
echo "[Merger] Merging formats into $carpeta/reel-prueba-$segundos.mp4" >&2
echo "POSTPROCESS|started|Merger" >&2
sleep 0.5
echo "POSTPROCESS|finished|Merger" >&2
mv "$parte" "$final"
# El final lo imprime `--print`, que si va por stdout.
echo "DONE|$final"
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
        let mut paso_visto: Option<String> = None;
        let mut finales: Option<State> = None;

        while Instant::now() < limite {
            backend.drain();
            {
                let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(job) = queue.jobs.first() {
                    match &job.state {
                        State::Queued => vio_en_espera = true,
                        State::Downloading => vio_descarga = true,
                        State::Postprocessing { .. } => {
                            vio_postproceso = true;
                            paso_visto = job.postprocessor.clone();
                        }
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
            "espera={vio_en_espera} descarga={vio_descarga} progreso={vio_progreso} postproceso={paso_visto:?} final={finales:?}"
        );
        assert!(vio_descarga, "nunca se vio el trabajo descargando");
        // El progreso y el postprocesado los manda yt-dlp por stderr: si se
        // vuelve a leer solo stdout, esto deja de llegar.
        assert!(
            vio_progreso,
            "nunca llego progreso: no se esta leyendo stderr"
        );
        assert!(
            vio_postproceso,
            "nunca se vio el postprocesado: no se estan leyendo los avisos de stderr"
        );
        assert_eq!(
            paso_visto.as_deref(),
            Some("Merger"),
            "el estado no llevaba el nombre del paso"
        );
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

    /// Una lista de verdad, para comprobar que se reconoce como lista y no
    /// como un video con titulo raro.
    const LISTA_DE_PRUEBA: &str =
        "https://www.youtube.com/playlist?list=PLbpi6ZahtOH6Blw3RGYpWkSByi_T7Rygb";

    /// Un video libre y real, pero diminuto (menos de 2 KB): alcanza para
    /// probar la invocacion completa sin bajar nada de peso. Los "mp4" de
    /// pocos bytes que tambien hay en el sitio no sirven: ffmpeg los rechaza al
    /// incrustar la caratula, y esa falla es del archivo, no de la app.
    const VIDEO_DE_PRUEBA: &str = "https://archive.org/details/stufffffff";

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
                    "probe OK: {:?} de {:?} en {}",
                    media.title, media.uploader, media.host
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
        assert_eq!(
            media.playlist_count, None,
            "un video suelto no puede leerse como lista"
        );

        // Y una lista de verdad, que es el caso que antes se colaba: se
        // encolaba la lista entera creyendo que era un video.
        match probe(LISTA_DE_PRUEBA) {
            Ok(lista) => {
                println!(
                    "lista OK: {:?} con {:?} videos",
                    lista.title, lista.playlist_count
                );
                let cuantos = lista
                    .playlist_count
                    .expect("una url de playlist tiene que reconocerse como lista");
                assert!(cuantos > 1, "una lista de {cuantos} no es una lista");
                assert_eq!(
                    lista.duration, None,
                    "la duracion de una lista no es la de un video"
                );
            }
            Err(reason) => {
                eprintln!("no pude leer la lista de prueba: {reason}");
                panic!("la deteccion de listas no se pudo probar");
            }
        }

        // Cada fase en su carpeta: compartirla hacia que una descarga anterior
        // dejara el archivo puesto y la siguiente se saltara el postprocesado,
        // que es justo lo que hay que mirar.
        let en_seco = salida.join("seco");
        let de_progreso = salida.join("progreso");
        let de_final = salida.join("final");
        for dir in [&en_seco, &de_progreso, &de_final] {
            std::fs::create_dir_all(dir).expect("deberia crear el directorio de la fase");
        }

        // Los mismos argumentos que arma la app, en seco: si un flag no existe
        // o el selector de formato es invalido, yt-dlp lo dice aca.
        let options = |dir: &Path| Options {
            output_dir: Some(dir.to_path_buf()),
            ..Options::default()
        };
        let (args, _) = download_args(VIDEO_DE_PRUEBA, &options(&en_seco));

        let mut con_seco = args.clone();
        con_seco.push("--skip-download".into());
        let seco = Proc::new(ytdlp_binary())
            .args(&con_seco)
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

        // La tuberia de progreso. Se corta apenas llegan lineas con la forma
        // esperada: no hace falta bajar todo para saber que se leen.
        let (args, _) = download_args(VIDEO_DE_PRUEBA, &options(&de_progreso));
        let mut hijo = Proc::new(ytdlp_binary())
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("deberia lanzar yt-dlp");

        let mut progresos = 0;
        {
            let stdout = hijo.stdout.take().expect("deberia tener stdout");
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if matches!(leer_linea(1, &line), Salida::Progreso(_)) {
                    progresos += 1;
                }
                if progresos >= 2 {
                    let _ = hijo.kill();
                    break;
                }
            }
        }
        // Se espera al hijo matado: si sigue vivo, todavia escribe.
        let _ = hijo.wait();
        assert!(
            progresos > 0,
            "yt-dlp no imprimio ninguna linea de progreso que supieramos leer"
        );

        // Y el postprocesado, con la descarga entera. El archivo de prueba pesa
        // menos de 2 KB, asi que esto no es trafico.
        let (args, _) = download_args(VIDEO_DE_PRUEBA, &options(&de_final));
        // Que la app de verdad este pidiendo las dos plantillas.
        let plantillas: Vec<&String> = args
            .iter()
            .enumerate()
            .filter(|(i, _)| *i > 0 && args[i - 1] == "--progress-template")
            .map(|(_, arg)| arg)
            .collect();
        println!("plantillas pedidas: {plantillas:?}");
        assert_eq!(plantillas.len(), 2, "la app no pide las dos plantillas");
        let mut hijo = Proc::new(ytdlp_binary())
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("deberia lanzar yt-dlp");

        let pasos: Vec<String> = Vec::new();
        let finales: Option<String> = None;
        let crudo: Vec<String> = Vec::new();
        // Las dos tuberias, como las lee la app. El progreso y los avisos de
        // postprocesado van por stderr: leer solo stdout era el bug.
        let pasos = Arc::new(Mutex::new(pasos));
        let finales = Arc::new(Mutex::new(finales));
        let crudo = Arc::new(Mutex::new(crudo));
        let mut lectores = Vec::new();
        let tuberias: [(Option<Box<dyn Read + Send>>, bool); 2] = [
            (
                hijo.stdout
                    .take()
                    .map(|s| Box::new(s) as Box<dyn Read + Send>),
                false,
            ),
            (
                hijo.stderr
                    .take()
                    .map(|s| Box::new(s) as Box<dyn Read + Send>),
                true,
            ),
        ];
        for (stream, guardar) in tuberias {
            let Some(stream) = stream else { continue };
            let pasos = Arc::clone(&pasos);
            let finales = Arc::clone(&finales);
            let crudo = Arc::clone(&crudo);
            lectores.push(std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    // Todo pasa por el mismo clasificador que usa la app: si algo
                    // no se cuenta aca, es porque la app tampoco lo cuenta.
                    match leer_linea(1, &line) {
                        Salida::Postprocesado(paso) => lock(&pasos).push(paso),
                        Salida::Terminado(path) => *lock(&finales) = Some(path),
                        _ => {}
                    }
                    if guardar {
                        let mut crudo = lock(&crudo);
                        crudo.push(line);
                        if crudo.len() > 30 {
                            crudo.remove(0);
                        }
                    }
                }
            }));
        }
        let estado = hijo.wait().expect("deberia esperar a yt-dlp");
        for lector in lectores {
            let _ = lector.join();
        }
        let pasos = Arc::try_unwrap(pasos)
            .map(|m| m.into_inner().unwrap_or_default())
            .unwrap_or_default();
        let finales = Arc::try_unwrap(finales)
            .map(|m| m.into_inner().unwrap_or_default())
            .unwrap_or_default();
        let crudo = Arc::try_unwrap(crudo)
            .map(|m| m.into_inner().unwrap_or_default())
            .unwrap_or_default();

        println!("pasos de postprocesado: {pasos:?}");
        println!("archivo final: {finales:?}");
        if !estado.success() {
            eprintln!("--- ultimas lineas de yt-dlp (stderr) ---");
            for line in &crudo {
                eprintln!("{line}");
            }
        }
        assert!(estado.success(), "yt-dlp termino en {estado:?}");
        assert!(
            finales.is_some(),
            "nunca llego la linea DONE: el --print no funciona con yt-dlp real"
        );
        assert!(
            finales
                .as_deref()
                .is_some_and(|path| Path::new(path).exists()),
            "el archivo que dijo yt-dlp no existe: {finales:?}"
        );
        assert!(
            pasos.iter().any(|paso| paso == "EmbedThumbnail"),
            "yt-dlp no aviso que estaba poniendo la caratula: la plantilla del postprocess no funciona ({pasos:?})"
        );
        println!(
            "OK descarga-real ({progresos} de progreso, {} de postprocesado)",
            pasos.len()
        );
    }

    /// Reintentar un trabajo fallado tiene que volver a pedirlo con los mismos
    /// argumentos, que es lo que hace que yt-dlp reanude el `.part` en vez de
    /// empezar de cero. El yt-dlp falso falla a proposito en el primer intento.
    fn comprobar_reintento(salida: &Path) {
        let guion = escribir_guion(salida).expect("deberia escribir el yt-dlp falso");
        let mut lanzador = std::fs::read_to_string(&guion).expect("deberia leerlo");
        // El falso falla en el primer intento solo si se lo piden.
        lanzador = lanzador.replace("fallar=0", "fallar=1");
        std::fs::write(&guion, &lanzador).expect("deberia reescribirlo");
        std::env::set_var("REEL_YTDLP", &guion);

        let backend = Backend::spawn(|| {});
        encolar(&backend, 1, "https://ejemplo.test/v1", salida);

        // Se espera a que falle.
        let limite = Instant::now() + Duration::from_secs(20);
        let mut fallo = false;
        while Instant::now() < limite && !fallo {
            backend.drain();
            let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
            fallo = queue
                .jobs
                .first()
                .is_some_and(|job| matches!(job.state, State::Failed { .. }));
        }
        assert!(fallo, "el primer intento deberia haber fallado");

        // Se reintenta desde la cola, como hace el boton.
        let pedido = {
            let mut queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.retry(1)
        };
        let (url, options) = pedido.expect("deberia poder reintentar");
        backend.send(Command::Start {
            id: 1,
            url,
            options,
        });

        let limite = Instant::now() + Duration::from_secs(20);
        let mut finales: Option<State> = None;
        while Instant::now() < limite && finales.is_none() {
            backend.drain();
            let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(job) = queue.jobs.first() {
                if !job.is_active() {
                    finales = Some(job.state.clone());
                }
            }
        }
        backend.send(Command::Shutdown);

        let reanudaciones =
            std::fs::read_to_string(salida.join("reanudaciones.txt")).unwrap_or_default();
        println!("final={finales:?}");
        println!("intentos:\n{reanudaciones}");
        assert!(
            matches!(finales, Some(State::Done { .. })),
            "el reintento no termino bien: {finales:?}"
        );
        // Dos intentos del mismo trabajo, y el segundo reusando el `.part`.
        let intentos: Vec<&str> = reanudaciones
            .lines()
            .filter(|line| line.starts_with("intento="))
            .collect();
        assert_eq!(
            intentos.len(),
            2,
            "no hubo exactamente dos intentos: {intentos:?}"
        );
        assert!(
            intentos[1].starts_with("intento=2"),
            "el reintento no conto como segundo intento: {intentos:?}"
        );
        println!("OK reintento");
    }

    /// Una lista se expande a una fila por video: la fila de la lista queda
    /// como resumen y cada video baja por su cuenta, con su progreso y su
    /// cancelacion.
    fn comprobar_expansion(salida: &Path) {
        let guion = escribir_guion(salida).expect("deberia escribir el yt-dlp falso");
        std::env::set_var("REEL_YTDLP", &guion);
        let backend = Backend::spawn(|| {});

        // La lista entra como un trabajo mas, marcado como lista.
        let mut options = Options {
            output_dir: Some(salida.to_path_buf()),
            ..Options::default()
        };
        options.playlist = true;
        let id = {
            let mut queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.push_ready(
                "https://ejemplo.test/lista".into(),
                options,
                Media {
                    title: "Una lista".into(),
                    playlist_count: Some(2),
                    ..Media::default()
                },
            )
        };
        backend.send(Command::Expandir {
            id,
            url: "https://ejemplo.test/lista".into(),
        });

        // La fila de la lista tiene que quedar como resumen y aparecer las dos
        // filas de video.
        let limite = Instant::now() + Duration::from_secs(20);
        let mut expandida = false;
        while Instant::now() < limite && !expandida {
            backend.drain();
            let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
            expandida = queue.jobs.len() == 3
                && queue
                    .jobs
                    .iter()
                    .find(|job| job.id == id)
                    .is_some_and(|job| matches!(job.state, State::Done { .. }));
        }
        assert!(expandida, "la lista no se expandio a una fila por video");

        // Las filas nuevas tienen que poder bajar: se les manda la orden, que
        // es lo que hace la app cuando `drain` se las devuelve.
        let nuevos: Vec<(u64, String, Options)> = {
            let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue
                .jobs
                .iter()
                .filter(|job| job.id != id)
                .map(|job| (job.id, job.url.clone(), job.options.clone()))
                .collect()
        };
        assert_eq!(nuevos.len(), 2, "deberian ser dos videos: {nuevos:?}");
        for (_, _, options) in &nuevos {
            assert!(!options.playlist, "un video de la lista no es una lista");
        }
        for (id, url, options) in nuevos {
            backend.send(Command::Start { id, url, options });
        }

        let limite = Instant::now() + Duration::from_secs(30);
        let mut finales = 0;
        while Instant::now() < limite && finales < 2 {
            backend.drain();
            let queue = backend.queue.lock().unwrap_or_else(|e| e.into_inner());
            finales = queue
                .jobs
                .iter()
                .filter(|job| job.id != id && matches!(job.state, State::Done { .. }))
                .count();
        }
        backend.send(Command::Shutdown);

        println!("filas={finales} de 2 terminadas");
        assert_eq!(finales, 2, "los videos de la lista no terminaron de bajar");
        println!("OK expansion");
    }

    /// Un archivo que quedo truncado se arregla con `volver a bajar`, y no con
    /// un reintento normal. Necesita red y el yt-dlp de verdad, porque lo que
    /// se prueba es justo lo que hace yt-dlp con un archivo que ya existe.
    fn comprobar_volver_a_bajar(salida: &Path) {
        std::env::remove_var("REEL_YTDLP");

        let video = salida.join("video");
        std::fs::create_dir_all(&video).expect("deberia crear la carpeta");
        // Un archivo con el nombre que va a buscar yt-dlp, pero roto.
        let roto = video.join("stufffffff.mov");
        std::fs::write(&roto, b"ARCHIVO CORRUPTO").expect("deberia escribir el roto");
        let antes = std::fs::metadata(&roto).expect("deberia medirlo").len();

        // Sin metadatos ni capitulos: lo que se prueba es el salteo de un
        // archivo existente, y el video de prueba no aguanta el postprocesado
        // (ffmpeg lo rechaza), que seria una falla por otro motivo.
        let options = Options {
            output_dir: Some(video.clone()),
            metadata: false,
            chapters: false,
            ..Options::default()
        };

        // Primero sin forzar: yt-dlp tiene que saltearlo y dejarlo roto, que es
        // el limite que esto viene a resolver.
        let (args, _) = download_args(VIDEO_DE_PRUEBA, &options);
        let salida_ytdlp = Proc::new(ytdlp_binary())
            .args(&args)
            .output()
            .expect("deberia lanzar yt-dlp");
        assert!(
            salida_ytdlp.status.success(),
            "yt-dlp fallo: {}",
            String::from_utf8_lossy(&salida_ytdlp.stderr)
        );
        let igual = std::fs::metadata(&roto).expect("deberia medirlo").len();
        println!("sin forzar: {antes} -> {igual} bytes (se espera igual)");
        assert_eq!(igual, antes, "sin forzar no deberia tocar el archivo");

        // Ahora forzando: tiene que bajarlo de nuevo y dejarlo entero.
        let forzado = Options {
            force: true,
            ..options
        };
        let (args, _) = download_args(VIDEO_DE_PRUEBA, &forzado);
        let salida_ytdlp = Proc::new(ytdlp_binary())
            .args(&args)
            .output()
            .expect("deberia lanzar yt-dlp");
        assert!(
            salida_ytdlp.status.success(),
            "yt-dlp fallo: {}",
            String::from_utf8_lossy(&salida_ytdlp.stderr)
        );

        let despues = std::fs::metadata(&roto).expect("deberia medirlo").len();
        println!("forzando:  {igual} -> {despues} bytes");
        assert!(
            despues > antes,
            "forzando deberia haber bajado el video de nuevo ({antes} -> {despues})"
        );
        println!("OK volver-a-bajar");
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
            "reintento" => comprobar_reintento(&salida),
            "expansion" => comprobar_expansion(&salida),
            "volver-a-bajar" => comprobar_volver_a_bajar(&salida),
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
    /// Los trabajos que acaba de crear una lista, para que la app les mande la
    /// orden de arrancar. Se vacia al leerlo.
    pub recien_encolados: Vec<u64>,
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
            postprocessor: None,
        });
        id
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    /// Mete los videos de una lista como trabajos propios, cada uno con las
    /// mismas opciones que la lista pero sin la marca de lista (si no, cada
    /// video intentaria expandirse otra vez).
    ///
    /// La fila de la lista queda como resumen: dice cuantos videos son y que ya
    /// estan abajo, en vez de bajar la lista entera como un solo trabajo.
    fn expandir_lista(&mut self, id: u64, videos: Vec<Media>, error: Option<String>) {
        if let Some(reason) = error {
            if let Some(job) = self.get_mut(id) {
                job.state = State::Failed {
                    reason: format!("no pude leer la lista: {reason}"),
                };
            }
            return;
        }

        let Some(job) = self.get_mut(id) else {
            return;
        };
        let mut options = job.options.clone();
        // Sin esto cada video intentaria expandir su propia lista.
        options.playlist = false;

        let mut ids = Vec::new();
        for video in videos {
            let url = video.url.clone();
            if url.is_empty() {
                continue;
            }
            ids.push(self.push_ready(url, options.clone(), video));
        }

        let cuantos = ids.len();
        if let Some(job) = self.get_mut(id) {
            job.progress = 1.0;
            job.state = State::Done {
                path: format!("{cuantos} videos abajo"),
            };
        }

        // Se devuelven para que la app les mande la orden de arrancar: el
        // worker no sabe de la cola, solo de procesos.
        self.recien_encolados = ids;
    }

    /// Como `retry`, pero pidiendo que se baje de nuevo aunque el archivo ya
    /// este. Es la salida cuando el que quedo esta truncado: `retry` lo
    /// saltearia y volveria a decir "listo" sobre el mismo archivo roto.
    ///
    /// La bandera va solo en lo que se devuelve para este intento, no en el
    /// trabajo: si quedara guardada, el reintento siguiente tambien bajaria de
    /// cero sin que nadie lo pida.
    pub fn retry_forzado(&mut self, id: u64) -> Option<(String, Options)> {
        let (url, mut options) = self.retry(id)?;
        options.force = true;
        Some((url, options))
    }

    /// Devuelve un trabajo terminado a la cola para volver a bajarlo.
    ///
    /// No toca la url, el formato ni la carpeta: los mismos argumentos son los
    /// que hacen que yt-dlp encuentre el `.part` y reanude en vez de empezar de
    /// cero. Los contadores vuelven a cero porque el progreso viejo ya no dice
    /// nada del intento nuevo.
    ///
    /// Devuelve los datos que hacen falta para volver a pedirlo, o `None` si el
    /// trabajo no esta para reintentar (por ejemplo, si ya esta bajando).
    pub fn retry(&mut self, id: u64) -> Option<(String, Options)> {
        let job = self.jobs.iter_mut().find(|j| j.id == id)?;
        if job.is_active() {
            return None;
        }

        job.state = State::Queued;
        job.progress = 0.0;
        job.speed = None;
        job.eta_secs = None;
        job.postprocessor = None;
        Some((job.url.clone(), job.options.clone()))
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
            Event::PlaylistExpandida { id, videos, error } => {
                // La fila de la lista queda como resumen y cada video es un
                // trabajo propio, con su progreso y su cancelacion.
                self.expandir_lista(id, videos, error);
            }
            Event::StateChanged { id, state } => {
                if let Some(job) = self.get_mut(id) {
                    if matches!(state, State::Done { .. }) {
                        job.progress = 1.0;
                    }
                    // El paso en curso se guarda en el trabajo: la fila lo
                    // cuenta y no se pierde si el estado cambia despues.
                    if let State::Postprocessing { postprocessor } = &state {
                        job.postprocessor = Some(postprocessor.clone());
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

    /// Vacia el canal de eventos sobre la cola.
    /// Vacia el canal de eventos. Devuelve los trabajos que creo una lista,
    /// que son los unicos que necesitan que alguien les mande la orden de
    /// arrancar: el worker no conoce la cola, solo procesos.
    pub fn drain(&self) -> (bool, Vec<u64>) {
        let mut changed = false;
        let mut queue = match self.queue.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        for event in self.events.try_iter() {
            queue.apply(event);
            changed = true;
        }
        (changed, std::mem::take(&mut queue.recien_encolados))
    }
}
