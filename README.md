# reel

Pegas un enlace, elegis un formato, y la cola hace el resto.

Una app de escritorio nativa para descargar video y audio, construida con
[egui](https://github.com/emilk/egui) sobre [fastframe](https://fastframe.dev).
El trabajo pesado lo hace [yt-dlp](https://github.com/yt-dlp/yt-dlp), que
soporta 1800 y pico de sitios, asi que esto no es solo YouTube.

> Estado: esqueleto. La interfaz y el cableado de fastframe estan puestos; la
> cola corre contra yt-dlp de verdad. Todavia no hay releases.

## Por que existe

[yoinks](https://github.com/pablostanley/yoinks) resolvio muy bien el gesto
basico en la terminal. Lo que le falta esta en sus issues abiertos: carpeta de
salida configurable, cookies del navegador para contenido con login, nombre de
archivo, capitulos, metadatos en los mp3, y nada de cola. reel toma ese gesto y
lo pone en una ventana donde la cola es la pantalla principal.

| | yoinks | plugin de barra | reel |
|---|---|---|---|
| Varias descargas a la vez | no | no | si, con progreso por item |
| Carpeta y nombre de salida | fijos | fijos | configurables |
| Cookies del navegador | no | no | si |
| Capitulos y subtitulos | no | no | si |
| Estado de postprocesado | invisible | invisible | visible |
| Sigue el tema del escritorio | no | si (en la barra) | si (Omarchy, en vivo) |

## Que pone fastframe

Casi todo lo que no es la interfaz. Esa es la idea de fastframe: se queda con
lo que es igual en toda app de escritorio, y la app se queda con su interfaz.

- `fastframe-fonts` y `fastframe-text`: Inter en cuatro pesos, fuentes
  instaladas para los alfabetos que Inter no cubre, y el hinting y
  antialiasing que use el escritorio. Esta en `src/fonts.rs`.
- `fastframe-theme`: paletas JSON, las ocho paletas compartidas, y seguir el
  tema de Omarchy en vivo con notificaciones del filesystem. La paleta de la
  app y su mapeo a `egui::Visuals` estan en `src/palette.rs`, que es justo lo
  que fastframe deja a cada app.
- `fastframe-shell`: la ventana se puede cerrar sin matar el proceso, asi que
  la cola sigue bajando desde el tray. `App` implementa `Resident`.
- `fastframe-tray`: el item de bandeja con su menu.
- `fastframe-icons`: los SVG incrustados, con el cargador que no olvida los
  bytes cuando egui recorta texturas.
- `fastframe-update`: autoactualizacion desde releases de GitHub con checksums
  firmados y rollback. En Arch, `installation()` se niega cuando la copia la
  maneja pacman o el AUR, que es lo correcto: ahi solo avisa.
- `fastframe-log`: log a stderr y a archivo para reportes de bugs, sin datos
  privados y sin el payload de los panics.

## Como se ve

![Mockup de la interfaz](assets/mockup.png)

## Construir

Necesitas Rust 1.98 o mas nuevo, y `yt-dlp` y `ffmpeg` en el PATH.

```sh
sudo pacman -S --needed yt-dlp ffmpeg wl-clipboard
cargo run
```

fastframe no esta en crates.io: las dependencias apuntan al tag `v0.2.2` del
repo de GitHub. El tag se mueve a mano y a proposito, porque las notas de cada
release dicen que hay que cambiar para subir.

Para apuntar a otro binario de yt-dlp:

```sh
REEL_YTDLP=/ruta/a/yt-dlp cargo run
```

## Estructura

```
src/
├── main.rs            arranque: log, shell, ventana
├── app.rs             estado, frame, e impl Resident
├── palette.rs         los colores de la app y su mapeo a egui
├── fonts.rs           tipografia
├── icon.rs            iconos e icono del tray
├── dirs.rs            config, estado, log
├── updates.rs         cuando revisar y como contarlo
├── backend/
│   ├── mod.rs         la cola, los formatos, los eventos
│   └── ytdlp.rs       el hilo que habla con yt-dlp
└── ui/
    ├── top_bar.rs     nombre, version, tema, ajustes
    ├── url_bar.rs     enlace, pegar, yoink
    ├── media_card.rs  lo detectado y sus formatos
    ├── queue.rs       la cola, que es la pantalla principal
    ├── status_bar.rs  carpeta, tema seguido, actualizacion
    └── widgets.rs     chips, barra de progreso, tarjetas
```

## Omarchy

`contrib/omarchy/reel.json.tpl` es la plantilla que Omarchy renderiza en cada
cambio de tema. En el primer arranque se copia a
`~/.config/omarchy/themed/reel.json.tpl` y el hook a
`~/.config/omarchy/hooks/theme-set.d/`, sin pisar nada que ya exista. De ahi en
adelante los colores cambian solos, con la revelacion desde el centro de la
ventana que hace el propio escritorio.

## Licencia

MIT. yt-dlp y ffmpeg se usan como programas externos, cada uno con la suya.

Descargar contenido puede violar los terminos de un sitio. Baja solo lo que
tenes derecho a guardar.
