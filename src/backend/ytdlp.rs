//! El hilo que habla con yt-dlp.
//!
//! Dos llamadas por trabajo: `-J --no-playlist` para resolver titulo, autor y
//! duracion, y luego la descarga con `--newline --progress-template` para que
//! el progreso llegue en lineas faciles de leer en vez del dibujo de barra.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command as Proc, Stdio};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use super::{format_by_id, Command, Event, Kind, Media, Options, State};

/// Plantilla de progreso: una linea por actualizacion, campos separados por
/// `|`, sin nada que parsear a ojo.
const PROGRESS_TEMPLATE: &str =
    "download:PROGRESS|%(progress._percent_str)s|%(progress.speed)s|%(progress.eta)s";

/// yt-dlp del PATH. Si algun dia se quiere el binario propio (como yoinks, que
/// lo baja a ~/.yoinks/bin), este es el unico lugar que cambia.
fn ytdlp_binary() -> PathBuf {
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

pub fn worker<W>(commands: Receiver<Command>, events: Sender<Event>, wake: W)
where
    W: Fn() + Send + 'static,
{
    let mut running: HashMap<u64, Child> = HashMap::new();
    // Las ordenes que sacamos del canal mientras buscabamos cancelaciones y
    // que todavia hay que atender. Sin esto, el Start que viaja detras de un
    // Probe se perdia y el trabajo se quedaba en espera para siempre.
    let mut pending: VecDeque<Command> = VecDeque::new();

    loop {
        let command = match pending.pop_front() {
            Some(command) => command,
            None => match commands.recv() {
                Ok(command) => command,
                Err(_) => break,
            },
        };

        match command {
            Command::Shutdown => {
                for (_, mut child) in running.drain() {
                    let _ = child.kill();
                }
                break;
            }
            Command::Cancel { id } => {
                if let Some(mut child) = running.remove(&id) {
                    let _ = child.kill();
                }
                let _ = events.send(Event::StateChanged {
                    id,
                    state: State::Cancelled,
                });
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
                let _ = events.send(Event::StateChanged {
                    id,
                    state: State::Downloading,
                });
                wake();
                download(id, &url, &options, &events, &wake, &mut running);
            }
        }

        // Las ordenes de cancelar que llegaron mientras descargabamos se
        // atienden ya; el resto se guarda en la fila y se atiende enseguida.
        loop {
            match commands.try_recv() {
                Ok(Command::Cancel { id }) => {
                    if let Some(mut child) = running.remove(&id) {
                        let _ = child.kill();
                    }
                }
                Ok(other) => pending.push_back(other),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
    }
}

fn probe(url: &str) -> Result<Media, String> {
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

fn download<W>(
    id: u64,
    url: &str,
    options: &Options,
    events: &Sender<Event>,
    wake: &W,
    running: &mut HashMap<u64, Child>,
) where
    W: Fn(),
{
    let format = format_by_id(&options.format_id);
    let dir = options
        .output_dir
        .clone()
        .unwrap_or_else(|| default_dir(format.kind));
    let template = options
        .filename_template
        .clone()
        .unwrap_or_else(|| "%(title).120s.%(ext)s".into());

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

    let child = Proc::new(ytdlp_binary())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            let _ = events.send(Event::StateChanged {
                id,
                state: State::Failed {
                    reason: format!("no se pudo ejecutar yt-dlp: {error}"),
                },
            });
            wake();
            return;
        }
    };

    let stdout = child.stdout.take();
    running.insert(id, child);

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

    let mut child = match running.remove(&id) {
        Some(child) => child,
        // Lo mato una cancelacion: el estado ya lo puso quien cancelo.
        None => return,
    };

    let status = child.wait();
    let state = match status {
        Ok(status) if status.success() => State::Done {
            path: final_path.unwrap_or_else(|| dir.display().to_string()),
        },
        Ok(_) => {
            let reason = child
                .stderr
                .take()
                .map(|stderr| {
                    let mut text = String::new();
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        text.push_str(&line);
                        text.push('\n');
                    }
                    first_useful_line(&text)
                })
                .unwrap_or_else(|| "yt-dlp termino con error".into());
            State::Failed { reason }
        }
        Err(error) => State::Failed {
            reason: format!("no pude esperar a yt-dlp: {error}"),
        },
    };

    let _ = events.send(Event::StateChanged { id, state });
    wake();
}

fn parse_progress(id: u64, rest: &str) -> Option<Event> {
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
}
