//! La cola en disco, en la carpeta de estado.
//!
//! Se reescribe entera cada vez que cambia. Un archivo que falta es un
//! estreno; uno ilegible se avisa y se arranca vacio, porque no abrir por un
//! JSON roto seria peor.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Job, Queue};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct ColaGuardada {
    jobs: Vec<Job>,
    next_id: u64,
}

impl Queue {
    /// Snapshot que se puede comparar y escribir: las filas y el proximo id.
    /// El preview y los recien encolados no duran entre arranques.
    pub fn snapshot(&self) -> Self {
        Self {
            jobs: self.jobs.clone(),
            preview: None,
            preview_error: None,
            recien_encolados: Vec::new(),
            next_id: self.next_id,
        }
    }

    /// Lee `queue.json`. Falta = cola vacia, sin ruido. Roto = cola vacia y
    /// un aviso en el log.
    pub fn load() -> Self {
        Self::load_from(&crate::dirs::queue_file())
    }

    pub fn load_from(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("no pude leer {}: {error}", path.display());
                }
                return Self::default();
            }
        };

        match serde_json::from_str::<ColaGuardada>(&text) {
            Ok(guardada) => Self::desde_guardada(guardada),
            Err(error) => {
                log::warn!(
                    "{} no se entiende, arranco con la cola vacia: {error}",
                    path.display()
                );
                Self::default()
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&crate::dirs::queue_file())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.como_guardada())
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        // tmp + rename: un cierre a medias no deja un JSON a medio escribir.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    fn como_guardada(&self) -> ColaGuardada {
        ColaGuardada {
            jobs: self.jobs.clone(),
            next_id: self.next_id,
        }
    }

    fn desde_guardada(guardada: ColaGuardada) -> Self {
        let max_id = guardada.jobs.iter().map(|job| job.id).max().unwrap_or(0);
        Self {
            jobs: guardada.jobs,
            preview: None,
            preview_error: None,
            recien_encolados: Vec::new(),
            next_id: guardada.next_id.max(max_id),
        }
    }

    /// Lo que hay que volver a pedir al reabrir: pendientes y a medias. Las
    /// filas terminadas y con error se dejan como estaban.
    ///
    /// El proceso anterior ya no existe, asi que "descargando" miente: se
    /// vuelve a `en espera` y se manda `Start` (o `Expandir` si es una lista).
    /// yt-dlp retoma el `.part` con `--continue`.
    pub fn preparar_reanudacion(&mut self) -> Vec<PedidoAlReabrir> {
        let mut pedidos = Vec::new();
        for job in &mut self.jobs {
            if !job.is_active() {
                continue;
            }
            job.state = super::State::Queued;
            job.progress = 0.0;
            job.speed = None;
            job.eta_secs = None;
            job.postprocessor = None;
            pedidos.push(PedidoAlReabrir {
                id: job.id,
                url: job.url.clone(),
                options: job.options.clone(),
            });
        }
        pedidos
    }
}

/// Un trabajo que al reabrir hay que volver a mandar al worker.
#[derive(Clone, Debug)]
pub struct PedidoAlReabrir {
    pub id: u64,
    pub url: String,
    pub options: super::Options,
}

impl PedidoAlReabrir {
    pub fn es_lista(&self) -> bool {
        self.options.playlist
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Media, Options, State};

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "reel-cola-{}-{}-{tag}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    fn trabajo(id: u64, state: State) -> Job {
        Job {
            id,
            url: format!("https://ejemplo.test/{id}"),
            media: Media {
                title: format!("video {id}"),
                ..Media::default()
            },
            options: Options {
                format_id: "720p".into(),
                ..Options::default()
            },
            state,
            progress: 0.4,
            speed: Some(1024.0),
            eta_secs: Some(12),
            postprocessor: None,
        }
    }

    #[test]
    fn guardar_y_recargar_conserva_los_estados() {
        let path = temp_path("roundtrip");
        let mut cola = Queue {
            jobs: vec![
                trabajo(
                    1,
                    State::Done {
                        path: "/tmp/listo.mp4".into(),
                    },
                ),
                trabajo(
                    2,
                    State::Failed {
                        reason: "403".into(),
                    },
                ),
                trabajo(3, State::Queued),
                trabajo(
                    4,
                    State::Retrying {
                        reason: "403".into(),
                        attempt: 1,
                        wait_ms: 60_000,
                    },
                ),
                trabajo(5, State::Downloading),
                trabajo(6, State::Cancelled),
            ],
            next_id: 6,
            preview: Some(("https://ejemplo.test/x".into(), Media::default())),
            recien_encolados: vec![9],
            ..Queue::default()
        };
        cola.jobs[0].progress = 1.0;
        cola.jobs[0].speed = None;
        cola.jobs[0].eta_secs = None;

        cola.save_to(&path).expect("deberia guardar");
        let mut recargada = Queue::load_from(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(recargada.jobs, cola.jobs);
        assert_eq!(recargada.next_id, 6);
        assert!(recargada.preview.is_none());
        assert!(recargada.recien_encolados.is_empty());
        assert!(matches!(recargada.jobs[1].state, State::Failed { .. }));
        assert!(
            !recargada.jobs[1].is_active(),
            "con error sigue reintentable"
        );

        assert!(
            recargada.jobs[3].is_active(),
            "esperando reintento sigue activo"
        );

        let pedidos = recargada.preparar_reanudacion();
        assert_eq!(pedidos.len(), 3, "pendiente, reintento y a medias arrancan");
        assert!(matches!(recargada.jobs[0].state, State::Done { .. }));
        assert!(matches!(recargada.jobs[1].state, State::Failed { .. }));
        assert_eq!(recargada.jobs[2].state, State::Queued);
        assert_eq!(recargada.jobs[3].state, State::Queued);
        assert_eq!(recargada.jobs[4].state, State::Queued);
        assert_eq!(recargada.jobs[5].state, State::Cancelled);
        assert!(pedidos.iter().any(|p| p.id == 3));
        assert!(pedidos.iter().any(|p| p.id == 4));
        assert!(pedidos.iter().any(|p| p.id == 5));
    }

    #[test]
    fn una_cola_danada_no_tumba_reel() {
        let path = temp_path("danada");
        std::fs::write(&path, "{ esto no es json").expect("deberia escribir basura");
        let cola = Queue::load_from(&path);
        let _ = std::fs::remove_file(&path);

        assert!(cola.jobs.is_empty());
        assert_eq!(cola.next_id, 0);
    }
}
