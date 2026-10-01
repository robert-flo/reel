//! Pruebas de la cola corriendo el binario de verdad, con un yt-dlp falso.
//!
//! Cada comprobacion vive en un proceso aparte porque toca `REEL_YTDLP`, que
//! es del entorno y no se puede cambiar sin pisarle el suelo a nadie.
//!
//! Solo corre con la feature puesta:
//!
//! ```sh
//! cargo test --features selfcheck
//! ```

#![cfg(feature = "selfcheck")]

use std::process::Command;

fn correr(modo: &str) -> (bool, String) {
    let binario = env!("CARGO_BIN_EXE_reel");
    let salida = Command::new(binario)
        .args(["--download-selfcheck", modo])
        .output()
        .unwrap_or_else(|error| panic!("no pude correr {binario}: {error}"));

    let texto = format!(
        "{}\n{}",
        String::from_utf8_lossy(&salida.stdout),
        String::from_utf8_lossy(&salida.stderr)
    );
    (salida.status.success(), texto)
}

fn comprobar(modo: &str) {
    let (paso, texto) = correr(modo);
    assert!(paso, "la prueba {modo} fallo:\n{texto}");
    // Que las comprobaciones de verdad se hayan corrido, no que el proceso
    // haya salido por la puerta de atras.
    assert!(
        texto.contains(&format!("OK {modo}")),
        "la prueba {modo} no llego al final:\n{texto}"
    );
    println!("{texto}");
}

#[test]
fn la_cola_baja_varios_a_la_vez() {
    comprobar("concurrencia");
}

#[test]
fn el_estado_de_la_fila_no_miente() {
    comprobar("estado");
}

#[test]
fn cancelar_mata_el_hijo_de_verdad() {
    comprobar("cancelacion");
}

#[test]
fn el_cupo_no_deja_lanzar_todo_a_la_vez() {
    comprobar("limite");
}

#[test]
fn una_prueba_desconocida_no_pasa_por_buena() {
    let (paso, texto) = correr("no-existe");
    assert!(!paso, "una prueba inexistente deberia fallar");
    assert!(texto.contains("desconocida"), "deberia decirlo: {texto}");
}
