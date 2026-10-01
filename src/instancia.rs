//! Una sola instancia de la app.
//!
//! fastframe-shell no trae esto a proposito: cada app elige su diseño. Aca se
//! usa un socket en el directorio de estado, que sirve para dos cosas: saber si
//! ya hay una instancia, y poder mandarle trabajo a la que corre.
//!
//! La segunda instancia no abre ventana ni un segundo icono en el tray: le pasa
//! su enlace a la primera y se va. Sin esto, dos copias pelearian por el mismo
//! item de bandeja y escribirian el mismo `app.ron`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};

use crate::dirs;

/// Lo que una instancia nueva le pide a la que ya corre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Aviso {
    /// Que muestre la ventana.
    Mostrar,
    /// Que lea este enlace y lo encole.
    Yoink(String),
}

/// El socket de la instancia que corre, si es que hay una.
fn socket_file() -> std::path::PathBuf {
    dirs::state_dir().join("reel.sock")
}

/// Se queda con el socket. `None` cuando ya hay otra instancia corriendo, que
/// es la senal de que esta copia tiene que avisarle y salir.
pub fn tomar() -> Option<UnixListener> {
    let path = socket_file();
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            log::warn!("no pude crear {}: {error}", parent.display());
            return None;
        }
    }

    // Un socket que quedo de un cierre feo no sirve para conectarse, asi que
    // se intenta primero conectar: si nadie contesta, es basura y se borra.
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            return None;
        }
        let _ = std::fs::remove_file(&path);
    }

    match UnixListener::bind(&path) {
        Ok(listener) => Some(listener),
        Err(error) => {
            // Sin poder quedarse con el socket, se sigue igual: es peor no
            // abrir la app que tener dos copias.
            log::warn!("no pude escuchar en {}: {error}", path.display());
            None
        }
    }
}

/// Le pasa el aviso a la instancia que corre. `false` si no hay ninguna
/// escuchando, que es cuando esta copia tiene que abrir su propia ventana.
pub fn avisar(aviso: &Aviso) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket_file()) else {
        return false;
    };
    let linea = match aviso {
        Aviso::Mostrar => "mostrar\n".to_string(),
        // El enlace puede traer cualquier cosa menos un salto de linea, que es
        // el separador.
        Aviso::Yoink(url) => format!("yoink {}\n", url.replace('\n', " ")),
    };
    stream.write_all(linea.as_bytes()).is_ok()
}

/// Escucha lo que le manden las instancias nuevas. Cada aviso va al canal que
/// lee la app.
pub fn escuchar(listener: UnixListener, mut al_recibir: impl FnMut(Aviso) + Send + 'static) {
    std::thread::Builder::new()
        .name("reel-avisos".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let Ok(linea) = BufReader::new(stream).lines().next().transpose() else {
                    continue;
                };
                let Some(linea) = linea else { continue };
                match leer_aviso(&linea) {
                    Some(aviso) => al_recibir(aviso),
                    None => log::warn!("un aviso que no entiendo: {linea:?}"),
                }
            }
        })
        .ok();
}

/// Una linea del socket como `Aviso`. Separado para poder probarlo.
fn leer_aviso(linea: &str) -> Option<Aviso> {
    let linea = linea.trim();
    if linea == "mostrar" {
        return Some(Aviso::Mostrar);
    }
    if let Some(url) = linea.strip_prefix("yoink ") {
        let url = url.trim();
        return (!url.is_empty()).then(|| Aviso::Yoink(url.to_string()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_los_avisos() {
        assert_eq!(leer_aviso("mostrar"), Some(Aviso::Mostrar));
        assert_eq!(leer_aviso("  mostrar  "), Some(Aviso::Mostrar));
        assert_eq!(
            leer_aviso("yoink https://ejemplo.test/v"),
            Some(Aviso::Yoink("https://ejemplo.test/v".into()))
        );
        // Un enlace con espacios igual se lee: se recorta.
        assert_eq!(
            leer_aviso("yoink  https://ejemplo.test/v  "),
            Some(Aviso::Yoink("https://ejemplo.test/v".into()))
        );
    }

    #[test]
    fn descarta_lo_que_no_entiende() {
        assert_eq!(leer_aviso(""), None);
        assert_eq!(leer_aviso("cualquier cosa"), None);
        assert_eq!(leer_aviso("yoink"), None);
        assert_eq!(leer_aviso("yoink   "), None);
        assert_eq!(leer_aviso("mostrar algo"), None);
    }
}
