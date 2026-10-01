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
        args: &[
            "-f",
            "ba/b",
            "-x",
            "--audio-format",
            "mp3",
            "--audio-quality",
            "0",
            // El issue 12 de yoinks, resuelto de entrada.
            "--embed-metadata",
            "--embed-thumbnail",
        ],
    },
    Format {
        id: "opus",
        label: "opus",
        kind: Kind::Audio,
        args: &[
            "-f",
            "ba/b",
            "-x",
            "--audio-format",
            "opus",
            "--embed-metadata",
        ],
    },
];

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
    Probe {
        id: u64,
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
    Probed {
        id: u64,
        media: Media,
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

/// La cola completa, compartida con la interfaz.
#[derive(Default)]
pub struct Queue {
    pub jobs: Vec<Job>,
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

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Probed { id, media } => {
                if let Some(job) = self.get_mut(id) {
                    job.media = media;
                }
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
        W: Fn() + Send + 'static,
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
