# reel

Pegas un enlace, elegis un formato, y la cola hace el resto.

Una app de escritorio nativa para descargar video y audio, construida con
[egui](https://github.com/emilk/egui) sobre [fastframe](https://fastframe.dev).
El trabajo pesado lo hace [yt-dlp](https://github.com/yt-dlp/yt-dlp), que
soporta 1800 y pico de sitios, asi que esto no es solo YouTube.

> Estado: esqueleto avanzado. La interfaz, el cableado de fastframe y el panel
> de ajustes estan puestos; la cola corre contra yt-dlp de verdad. Todavia no
> hay releases.

## Ajustes

El boton `ajustes` de la esquina, o `reel --settings`, abre el panel. Lo que
se elige ahi se guarda en `~/.config/reel/settings.json` y sobrevive al cierre:

- **Carpeta de salida**: vacia usa `~/Videos` para video y `~/Music` para
  audio, que es lo que elige yt-dlp. Acepta `~` y `$HOME`, y avisa si la
  carpeta no existe, es un archivo o es de solo lectura.
- **Formato**: el que se va a bajar. Se elige en la ficha cuando hay un enlace
  pegado, pero tambien es un ajuste: el que queda es el que arranca la proxima
  vez, en vez de volver siempre a "Mejor".
- **Nombre del archivo**: la plantilla de `-o` de yt-dlp. Vacia usa
  `%(title).120s.%(ext)s`.
- **Cookies del navegador**: `--cookies-from-browser`, para contenido con
  sesion. La lista sale de los navegadores que hay en la maquina, y el que
  viene marcado es el que el escritorio tiene por defecto.
- **Subtitulos**: los idiomas que se bajan y se incrustan. Los comunes
  (`es`, `en`, `pt`, `fr`) son un toque y los demas se escriben a mano como
  `de, it`, que es lo que termina en `--sub-langs`.
- **Tema**: seguir el tema de Omarchy en vivo, o clavar una de las paletas
  compartidas. La eleccion tambien se recuerda.

## La cola

Varios trabajos bajan a la vez, cada uno en su hilo, y la fila dice lo que
esta pasando de verdad: `en espera` hasta que yt-dlp arranca, `descargando`
con velocidad y tiempo restante, `esperando ffmpeg` mientras se unen las
pistas, y `listo` con el archivo que quedo. Los que ya terminaron ofrecen
`abrir carpeta`.

## La ventana

Se abre centrada la primera vez, con el icono de la app (el mismo del tray) y
1200x780. eframe guarda su estado en `~/.local/share/reel/app.ron`, asi que el
tamano que dejo el usuario se repone en el arranque siguiente.

Ojo con lo que eso significa en Wayland: el compositor manda. En Hyprland, que
es de mosaico, la ventana ocupa lo que le toca y ni la posicion ni el tamano
que guardemos se aplican; el estado sirve sobre todo para la memoria de egui.
En X11 y en ventanas flotantes si se repone donde estaba.

El aviso de "ventana fuera de pantalla" se comprueba en cada ventana y no una
sola vez: fastframe-shell vuelve a crear la ventana cada vez que se muestra
desde el tray.

Al arrancar, la app le pregunta la version a `yt-dlp` en un hilo. Si anda, el
pie lo dice (`yt-dlp 2026.08.19`); si falta o el binario del PATH no es yt-dlp,
avisa ahi mismo en ambar con el motivo en el hover, en vez de dejar que el
usuario lo descubra cuando ya apreto "descargar".

Cuando la cola deja de tener trabajo, la app avisa por el escritorio con
`notify-send`. Avisa una vez, no por archivo: al encolar diez enlaces, diez
notificaciones serian una lluvia justo cuando el usuario esta mirando la
ventana.

El paso de postprocesado lo cuenta yt-dlp con su `postprocess:`
`--progress-template`, no adivinando sus mensajes: unir pistas, extraer el
audio y poner la caratula salen de ahi. Antes se buscaba `[Merger]` en el
texto, lo que tenia dos problemas: si yt-dlp cambiaba el mensaje la fila dejaba
de avisar en silencio, y ademas esos mensajes van por stderr, que no se leia.

Corren como mucho `MAX_CONCURRENTES` (tres) a la vez. Lo que sobre espera su
lugar en vez de lanzar treinta yt-dlp y treinta ffmpeg contra la maquina.
Cancelar mata el proceso de verdad y deja el trabajo en `cancelado`, no en
`listo`.

Todo eso se prueba sin bajar nada y sin red, con un yt-dlp de mentira:

```sh
make selfcheck
```

Mide que dos trabajos se solapen, que el estado de la fila no mienta, que el
tope se respete, que cancelar mate al hijo y que nadie pase de "cancelado" a
"listo".

Antes de un commit, `make verify` corre el formato, clippy con los warnings
como errores y todas las pruebas. `make selfcheck` corre solo las de la cola.

`make selfcheck-net` es la unica que sale a internet: usa el yt-dlp de verdad
para comprobar el contrato que las demas no pueden, porque el yt-dlp de mentira
acepta cualquier flag. Lee los metadatos de un video libre, le pasa los mismos
argumentos que armaria la app (en seco, con `--skip-download`) y confirma que
el progreso llega con la forma que la cola sabe leer.

## Por que existe

[yoinks](https://github.com/pablostanley/yoinks) resolvio muy bien el gesto
basico en la terminal. Lo que le falta esta en sus issues abiertos: carpeta de
salida configurable, cookies del navegador para contenido con login, nombre de
archivo, capitulos, metadatos en los mp3, y nada de cola. reel toma ese gesto y
lo pone en una ventana donde la cola es la pantalla principal.

| | yoinks | plugin de barra | reel |
|---|---|---|---|
| Varias descargas a la vez | no | no | si, hasta 3, con progreso por item |
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

Y para abrir la app con el panel de ajustes ya puesto:

```sh
cargo run -- --settings
```

## Estructura

```
src/
├── main.rs            arranque: log, shell, ventana
├── app.rs             estado, frame, e impl Resident
├── settings.rs        los ajustes que duran, y su archivo
├── palette.rs         los colores de la app y su mapeo a egui
├── fonts.rs           tipografia
├── icon.rs            iconos e icono del tray
├── dirs.rs            config, estado, log
├── updates.rs         cuando revisar y como contarlo
├── backend/
│   ├── mod.rs         la cola, los formatos, los eventos
│   └── ytdlp.rs       el hilo que habla con yt-dlp
└── ui/
    ├── top_bar.rs     nombre, version, ajustes
    ├── url_bar.rs     enlace, pegar, yoink
    ├── media_card.rs  lo detectado y sus formatos
    ├── queue.rs       la cola, que es la pantalla principal
    ├── settings.rs    el panel de ajustes y el selector de tema
    ├── status_bar.rs  carpeta, tema seguido, actualizacion
    └── widgets.rs     chips, barra de progreso, tarjetas
tests/
└── cola.rs            la cola a procesos reales, con un yt-dlp falso
```

`backend/ytdlp.rs` trae, detras de la feature `selfcheck`, un `yt-dlp` de
mentira y las comprobaciones que corren en un proceso aparte. No va en el
binario normal: `cargo build` sin la feature no lo incluye.

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
