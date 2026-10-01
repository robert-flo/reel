# reel

Pegas un enlace, elegis un formato, y la cola hace el resto.

Una app de escritorio nativa para descargar video y audio, construida con
[egui](https://github.com/emilk/egui) sobre [fastframe](https://fastframe.dev).
El trabajo pesado lo hace [yt-dlp](https://github.com/yt-dlp/yt-dlp), que
soporta 1800 y pico de sitios, asi que esto no es solo YouTube.

> Estado: esqueleto avanzado. La interfaz, el cableado de fastframe, el panel de
> ajustes y la cola concurrente estan puestos y probados contra yt-dlp de
> verdad. Todavia no hay releases.

## Lo que hace

- **La cola es la pantalla principal**, no un detalle. Hasta tres descargas a la
  vez, cada una con su progreso, su velocidad y su tiempo restante.
- **La fila dice la verdad**: `en espera` hasta que yt-dlp arranca de verdad,
  `descargando`, `esperando ffmpeg` mientras une pistas o extrae audio, y `listo`
  con el archivo que quedo.
- **Cancelar mata el proceso** y deja el trabajo en `cancelado`, no en `listo`.
  Lo bajado queda en un `.part`, asi que **reintentar reanuda** donde iba.
- **Ajustes que duran**: carpeta de salida, formato, plantilla del nombre,
  cookies del navegador, subtitulos y tema, guardados en disco.
- **Sigue el tema de Omarchy en vivo**, con la revelacion desde el centro que
  hace el propio escritorio.
- **Vive en el tray**: cerrar la ventana no mata la cola.
- **Avisa cuando termina**, una vez al vaciarse la cola, no una por archivo.

## Lo basico

1. Pega el enlace y apreta `buscar` (o Enter). La app lee el enlace y muestra la
   ficha con el titulo, el autor, la duracion y los formatos.
2. Elegi el formato en la ficha: `Mejor`, `4K`, `1080p`, `720p`, `mp3` u `opus`.
3. Ajusta las opciones de la ficha si hace falta: `capitulos`,
   `metadatos + caratula` y `subtitulos`.
4. `descargar` lo manda a la cola. La ficha se queda puesta, asi que podes
   encolar otra calidad del mismo enlace.

Si estas en otro lado y queres mandar algo directo, el menu del tray tiene
`Pegar y descargar`: agarra el portapapeles, lee el enlace y lo encola solo.

### Listas de reproduccion

Si el enlace es una lista y no un video, la ficha lo dice: `es una lista: 19
videos`, y el boton cambia a `encolar 19`. Vale avisarlo porque
`--no-playlist` **no** frena una url de lista: yt-dlp la baja entera.

Una lista de **diez videos o mas** se confirma en dos toques: el primero cambia
el boton a `confirmar 19` y el segundo encola. Es a proposito: una lista de 19
videos de YouTube son gigabytes, y encolarla no deberia salir de un clic que
quiza se queria dar en otro lado. Con menos de diez, encolarla es barato y
preguntar solo molesta.

Al encolarla, la lista **se expande a una fila por video**: se pide el listado
con `--flat-playlist` (que no baja nada, solo los titulos y las duraciones) y
cada video entra como un trabajo propio, con su progreso, su cancelacion y su
reintento. La fila de la lista queda arriba como resumen y termina diciendo
cuantos videos encolo. Si un video de la lista falla, los demas siguen.

### Los formatos

| Formato | Que baja |
|---|---|
| `Mejor` | el mejor video con el mejor audio, en mp4 |
| `4K` / `1080p` / `720p` | lo mejor hasta esa altura, en mp4 |
| `mp3` | solo audio, extraido y con calidad 0 |
| `opus` | solo audio, en opus |

`metadatos + caratula` agrega los datos del video y la miniatura al archivo.
`capitulos` los incrusta (solo video).

`Mejor` y las calidades de video piden el mejor par de pistas que ofrezca el
sitio y las unen en mp4. Con YouTube eso suele dar **av1 de video y opus de
audio**, que es lo mejor que hay pero no lo abre cualquier reproductor viejo.
Si el archivo va a un televisor o a un telefono que no los soporte, elegi una
altura concreta (`720p`) o bajalo en `mp3`. Los subtitulos se bajan y se incrustan en
los idiomas que elijas en los ajustes.

## La cola

Cada fila trae el titulo, el formato elegido, el estado y la barra de progreso.
Debajo del estado, cuando corresponde, la velocidad y el tiempo restante.

- **Cancelar** corta el trabajo. El proceso se mata de verdad y lo bajado queda
  en disco.
- **Reintentar** vuelve a pedir el trabajo con los mismos argumentos, que es lo
  que hace que yt-dlp **reanude** el `.part` en vez de empezar de cero. Esta en
  cada fila terminada y, cuando hay varios, como `reintentar todo` en la
  cabecera: es lo que uno quiere despues de que se caiga la red.

  Un detalle que conviene saber: yt-dlp **no vuelve a bajar** un archivo que ya
  esta en la carpeta de salida, ni siquiera al reintentar. Para el caso normal
  esta bien, porque reintentar termina rapido y la fila queda en `listo`. Pero
  si el archivo quedo truncado o corrupto —un postprocesado que fallo a
  medias—, el reintento no lo arregla: hay que borrarlo a mano y volver a
  pedirlo.
- **Abrir carpeta** abre donde quedo el archivo.

### Cuantos a la vez

Como mucho `MAX_CONCURRENTES` (tres). Lo que sobra espera su lugar en vez de
lanzar treinta yt-dlp y treinta ffmpeg contra la maquina. El tope esta en
`src/backend/ytdlp.rs` si lo queres cambiar.

## Ajustes

El boton `ajustes` de la esquina, o `reel --settings`, abre el panel. Lo que se
elige ahi se guarda en `~/.config/reel/settings.json` y sobrevive al cierre:

- **Carpeta de salida**: vacia usa `~/Videos` para video y `~/Music` para audio,
  que es lo que elige yt-dlp. Acepta `~` y `$HOME`, y avisa si la carpeta no
  existe, es un archivo o es de solo lectura.
- **Formato**: el que se va a bajar. Se elige en la ficha cuando hay un enlace
  pegado, pero tambien es un ajuste: el que queda es el que arranca la proxima
  vez, en vez de volver siempre a `Mejor`.
- **Nombre del archivo**: la plantilla de `-o` de yt-dlp. Vacia usa
  `%(title).120s.%(ext)s`.
- **Subtitulos**: los idiomas que se bajan y se incrustan. Los comunes (`es`,
  `en`, `pt`, `fr`) son un toque y los demas se escriben a mano como `de, it`,
  que es lo que termina en `--sub-langs`.
- **Cookies del navegador**: `--cookies-from-browser`, para contenido con
  sesion. La lista sale de los navegadores que hay en la maquina, y el que viene
  marcado es el que el escritorio tiene por defecto.
- **Tema**: seguir el tema de Omarchy en vivo, o clavar una de las paletas
  compartidas, con muestra de colores. La eleccion tambien se recuerda.

Los campos de texto se confirman al salir del campo o al cerrar el panel, para
no validar una ruta a medio tipear. Un `settings.json` roto avisa al log y la
app arranca con los valores por defecto en vez de no abrir.

## La ventana

Se abre centrada la primera vez, con el icono de la app (el mismo del tray) y
1200x780. eframe guarda su estado en `~/.local/share/reel/app.ron`, asi que el
tamano que dejo el usuario se repone en el arranque siguiente.

Ojo con lo que eso significa en Wayland: **el compositor manda**. En Hyprland,
que es de mosaico, la ventana ocupa lo que le toca y ni la posicion ni el tamano
que guardemos se aplican; el estado sirve sobre todo para la memoria de egui. En
X11 y en ventanas flotantes si se repone donde estaba.

El aviso de "ventana fuera de pantalla" se comprueba en cada ventana y no una
sola vez: fastframe-shell vuelve a crear la ventana cada vez que se muestra
desde el tray.

## Cuando algo no anda

Al arrancar, la app le pregunta la version a `yt-dlp` en un hilo. Si anda, el
pie lo dice (`yt-dlp 2026.08.19`); si falta o el binario del PATH no es yt-dlp,
avisa ahi mismo en ambar con el motivo en el hover, en vez de dejar que lo
descubras cuando ya apretaste `descargar`.

Cuando una descarga falla, la fila muestra el error de yt-dlp tal cual —es la
unica forma de reportarlo— y, si es uno de los que se repiten, tambien que
hacer: el `403` de YouTube avisa que suele ser por pedir muchas veces seguidas
y sugiere reintentar en un rato; un video privado sugiere las cookies del
navegador; un formato que no existe sugiere elegir otro.

`make doctor` dice que falta en el sistema para que la app funcione.

El log y el registro de pánicos quedan en `~/.local/state/reel/`.

## Por que existe

[yoinks](https://github.com/pablostanley/yoinks) resolvio muy bien el gesto
basico en la terminal. Lo que le falta esta en sus issues abiertos: carpeta de
salida configurable, cookies del navegador para contenido con login, nombre de
archivo, capitulos, metadatos en los mp3, y nada de cola. reel toma ese gesto y
lo pone en una ventana donde la cola es la pantalla principal.

| | yoinks | plugin de barra | reel |
|---|---|---|---|
| Varias descargas a la vez | no | no | si, hasta 3, con progreso por item |
| Listas de reproduccion | no | no | si, una fila por video |
| Reanudar lo cortado | no | no | si, reintentar reanuda el `.part` |
| Carpeta y nombre de salida | fijos | fijos | configurables |
| Cookies del navegador | no | no | si |
| Subtitulos | no | no | si, con los idiomas que elijas |
| Estado de postprocesado | invisible | invisible | visible, con el paso que corre |
| Sigue el tema del escritorio | no | si (en la barra) | si (Omarchy, en vivo) |
| Vive en el tray | no | si | si, con la cola corriendo |

## Donde guarda las cosas

| Que | Donde |
|---|---|
| Ajustes | `~/.config/reel/settings.json` |
| Temas propios | `~/.config/reel/themes/` |
| Estado de la ventana | `~/.local/share/reel/app.ron` |
| Log y pánicos | `~/.local/state/reel/` |
| Socket de instancia | `~/.local/state/reel/reel.sock` |
| Lo bajado | `~/Videos` y `~/Music`, o lo que elijas |

## Construir

Necesitas Rust 1.98 o mas nuevo, y `yt-dlp` y `ffmpeg` en el PATH.

```sh
sudo pacman -S --needed yt-dlp ffmpeg wl-clipboard
cargo run
```

fastframe no esta en crates.io: las dependencias apuntan al tag `v0.2.2` del
repo de GitHub. El tag se mueve a mano y a proposito, porque las notas de cada
release dicen que hay que cambiar para subir.

`eframe` trae winit por dentro, asi que la ventana se maneja con su API y winit
no se declara como dependencia propia. Se le pide la feature `persistence` para
que recuerde el estado.

Para apuntar a otro binario de yt-dlp:

```sh
REEL_YTDLP=/ruta/a/yt-dlp cargo run
```

Y para abrir la app con el panel de ajustes ya puesto:

```sh
cargo run -- --settings
```

Para mandar un enlace directo a la cola, sin abrir la ficha ni apretar nada:

```sh
cargo run -- --yoink "https://..."
```

Es el mismo camino que `Pegar y descargar` del tray, asi que sirve para un
atajo del escritorio o para llamarlo desde un script.

**Solo hay una instancia.** Si la app ya esta abierta y la volves a lanzar, la
nueva no abre una segunda ventana: le pasa su pedido a la que corre y se va.
Con `--yoink` le manda el enlace, asi que esto funciona aunque la ventana este
cerrada en el tray:

```sh
reel --yoink "https://..."   # se lo encola a la que ya corre
```

Sin esto, dos copias pelearian por el mismo icono de bandeja y escribirian el
mismo archivo de estado.

## Como se prueba

Antes de un commit, `make verify` corre el formato, clippy con los warnings como
errores y todas las pruebas:

```sh
make verify
```

Las pruebas de la cola corren **sin red y sin bajar nada**, con un yt-dlp de
mentira, detras de la feature `selfcheck`:

```sh
make selfcheck
```

Miden que dos trabajos se solapen, que el estado de la fila no mienta, que el
tope se respete, que cancelar mate al hijo, que un trabajo fallado se pueda
reintentar, que una lista se expanda a una fila por video y que nadie pase de
"cancelado" a "listo".

`make selfcheck-net` es la unica que sale a internet. Usa el yt-dlp de verdad
para comprobar el contrato que las demas no pueden, porque el de mentira acepta
cualquier flag: lee los metadatos de un video libre, le pasa los mismos
argumentos que armaria la app y confirma que el progreso y los avisos de
postprocesado llegan con la forma que la cola sabe leer. Tambien comprueba que
una lista de reproduccion se reconozca como lista y no como un video con titulo
raro.

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
│   └── ytdlp.rs       los hilos que hablan con yt-dlp
└── ui/
    ├── top_bar.rs     nombre, version, ajustes
    ├── url_bar.rs     enlace, pegar, buscar
    ├── media_card.rs  lo detectado y sus formatos
    ├── queue.rs       la cola, que es la pantalla principal
    ├── settings.rs    el panel de ajustes y el selector de tema
    ├── status_bar.rs  carpeta, yt-dlp, tema y actualizacion
    └── widgets.rs     chips, barra de progreso
tests/
└── cola.rs            la cola a procesos reales, con un yt-dlp falso
```

`backend/ytdlp.rs` trae, detras de la feature `selfcheck`, un `yt-dlp` de
mentira y las comprobaciones que corren en un proceso aparte. No va en el
binario normal: `cargo build` sin la feature no lo incluye.

## Que pone fastframe

Casi todo lo que no es la interfaz. Esa es la idea de fastframe: se queda con lo
que es igual en toda app de escritorio, y la app se queda con su interfaz.

- `fastframe-fonts` y `fastframe-text`: Inter en cuatro pesos, fuentes instaladas
  para los alfabetos que Inter no cubre, y el hinting y antialiasing que use el
  escritorio. Esta en `src/fonts.rs`.
- `fastframe-theme`: paletas JSON, las ocho paletas compartidas, y seguir el tema
  de Omarchy en vivo con notificaciones del filesystem. La paleta de la app y su
  mapeo a `egui::Visuals` estan en `src/palette.rs`, que es justo lo que
  fastframe deja a cada app.
- `fastframe-shell`: la ventana se puede cerrar sin matar el proceso, asi que la
  cola sigue bajando desde el tray. `App` implementa `Resident`.
- `fastframe-tray`: el item de bandeja con su menu.
- `fastframe-icons`: los SVG incrustados, con el cargador que no olvida los bytes
  cuando egui recorta texturas.
- `fastframe-update`: autoactualizacion desde releases de GitHub con checksums
  firmados y rollback. En Arch, `installation()` se niega cuando la copia la
  maneja pacman o el AUR, que es lo correcto: ahi solo avisa.
- `fastframe-log`: log a stderr y a archivo para reportes de bugs, sin datos
  privados y sin el payload de los panics.

## Omarchy

`contrib/omarchy/reel.json.tpl` es la plantilla que Omarchy renderiza en cada
cambio de tema. En el primer arranque se copia a
`~/.config/omarchy/themed/reel.json.tpl` y el hook a
`~/.config/omarchy/hooks/theme-set.d/`, sin pisar nada que ya exista. De ahi en
adelante los colores cambian solos, con la revelacion desde el centro de la
ventana que hace el propio escritorio.

## Como se ve

![Mockup de la interfaz](assets/mockup.png)

## Licencia

MIT. yt-dlp y ffmpeg se usan como programas externos, cada uno con la suya.

Descargar contenido puede violar los terminos de un sitio. Baja solo lo que
tenes derecho a guardar.
