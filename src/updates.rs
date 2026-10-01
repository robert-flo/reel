//! Autoactualizacion desde releases de GitHub, con checksums firmados y
//! rollback si la version nueva no arranca. Todo el trabajo lo hace
//! fastframe-update; aqui solo queda lo que la app decide: cuando revisar y
//! como se cuenta en la interfaz.

use std::sync::mpsc::Sender;

use fastframe_update::{MacConfig, ReqwestTransport, UpdateConfig, Updater};

use crate::app::{UpdateMessage, UpdateState};

pub const UPDATES: UpdateConfig = UpdateConfig {
    legacy_names: &[],
    macos: MacConfig {
        bundle_ids: &["dev.robertflo.reel"],
        executable_names: &[],
        legacy_bundle_names: &[],
    },
    publisher_key: None,
    ..UpdateConfig::new("robert-flo/reel", "reel", "reel", env!("CARGO_PKG_VERSION"))
};

/// `Updater` guarda el transporte detras de un objeto, asi que no lleva
/// parametro de tipo. El cliente de reqwest es nuestro: de ahi salen el
/// proxy y el TLS de la app.
fn updater() -> anyhow::Result<Updater> {
    let transport = ReqwestTransport::new(reqwest_builder())?;
    Ok(Updater::new(UPDATES, transport))
}

fn reqwest_builder() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder().user_agent(concat!("reel/", env!("CARGO_PKG_VERSION")))
}

/// Revisa una vez, en un hilo aparte. La app decide cuando llamarlo; el
/// intervalo de fastframe (`CHECK_INTERVAL`) es un dia.
pub fn check(tx: Sender<UpdateMessage>) {
    std::thread::spawn(move || {
        let _ = tx.send(UpdateMessage::State(UpdateState::Checking));

        let state = match updater().and_then(|updater| updater.check()) {
            Ok(Some(release)) => UpdateState::Available {
                version: release.version.clone(),
            },
            Ok(None) => UpdateState::Idle,
            // Mientras no haya releases publicados, GitHub contesta 404. No es
            // una falla que valga la pena contar en rojo: se distingue para
            // poder decir "todavia no hay versiones" en vez de "no pude".
            Err(error) if sin_releases(&error) => UpdateState::SinReleases,
            Err(error) => UpdateState::Failed(format!("no pude revisar: {error}")),
        };

        let _ = tx.send(UpdateMessage::State(state));
    });
}

/// Si el error es "este repositorio no tiene releases", que es lo normal
/// mientras no se publique ninguna. Se mira el texto porque el updater no da
/// un tipo para esto, y el 404 de la API de GitHub es lo unico que aparece en
/// este caso.
fn sin_releases(error: &anyhow::Error) -> bool {
    let texto = format!("{error:#}").to_lowercase();
    texto.contains("404") || texto.contains("not found")
}

/// Descarga y verifica. `installation()` se niega cuando la copia la maneja
/// un gestor de paquetes (pacman y el AUR incluidos), que en Arch es el caso
/// normal: ahi la app dice que hay version nueva y manda al release.
pub fn download(tx: Sender<UpdateMessage>) {
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<UpdateState> {
            let updater = updater()?;
            let Some(release) = updater.check()? else {
                return Ok(UpdateState::Idle);
            };
            if let Err(reason) = updater.installation() {
                return Ok(UpdateState::Unsupported(reason.to_string()));
            }

            let tx = tx.clone();
            updater.download(&release, |received, total| {
                let _ = tx.send(UpdateMessage::State(UpdateState::Downloading {
                    received,
                    total,
                }));
            })?;

            Ok(UpdateState::Ready)
        })();

        let state = result.unwrap_or_else(|error| {
            UpdateState::Failed(format!("la actualizacion fallo: {error}"))
        });
        let _ = tx.send(UpdateMessage::State(state));
    });
}

/// Le pasa el paquete al ayudante, que instala, relanza y revierte si la
/// version nueva no arranca. Despues de esto la app se cierra.
pub fn handoff() {
    log::info!("entregando la actualizacion al ayudante");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un repositorio sin releases contesta 404, y eso no es una falla: es que
    /// todavia no hay nada publicado.
    #[test]
    fn reconoce_un_repositorio_sin_releases() {
        let error = anyhow::anyhow!("no pude revisar: HTTP 404 Not Found");
        assert!(sin_releases(&error));

        let error = anyhow::anyhow!("404");
        assert!(sin_releases(&error));
    }

    /// Un problema de red o un JSON raro si son fallas que valga contar.
    #[test]
    fn no_confunde_otras_fallas_con_un_404() {
        let error = anyhow::anyhow!("no pude revisar: connection refused");
        assert!(!sin_releases(&error));

        let error = anyhow::anyhow!("no pude revisar: expected value at line 1");
        assert!(!sin_releases(&error));
    }
}
