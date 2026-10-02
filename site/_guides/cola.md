---
title: La cola
description: Progreso, cancelar, reintentar, volver a bajar, abrir carpeta.
nav_order: 3
---

# La cola

La cola es la pantalla principal. Hasta tres descargas a la vez; lo que
sobra espera. Cada fila tiene titulo, formato, estado, barra, y las
acciones que correspondan.

## Mientras baja

`descargar` limpia el campo de arriba y deja una fila. El estado nace
en `en espera` y pasa a `descargando` cuando yt-dlp arranca de verdad.
Si hay velocidad y tiempo restante, van debajo.

![Fila en descargando con el boton cancelar]({{ '/images/cola-descargando.png' | relative_url }})

`cancelar` mata el proceso. Lo bajado queda en un `.part`, no se marca
como `listo`.

Los contadores de la derecha (`1 activa`, `0 completadas`) van con la
cola, no con la ficha.

## Esperando ffmpeg

Cuando yt-dlp termina de bajar bytes y pasa a unir pistas, extraer
audio o incrustar la caratula, la fila dice `esperando ffmpeg` y el
paso concreto (`fusionando pistas`, `extrayendo el audio`,
`poniendo la caratula`). La barra se pone ambar.

![Fila en esperando ffmpeg, poniendo la caratula]({{ '/images/cola-ffmpeg.png' | relative_url }})

Sin ffmpeg, `Mejor`, `1080p` y `mp3` se caen al final. Por eso el pie
avisa al arrancar si falta.

## Listo

Cuando termina, `listo`, la ruta del archivo, y tres acciones:

![Fila lista con abrir carpeta, volver a bajar y reintentar]({{ '/images/cola-listo.png' | relative_url }})

- **abrir carpeta**: `xdg-open` en el directorio del archivo.
- **volver a bajar**: lo pide **de cero**, aunque el archivo ya este.
  Sirve si quedo truncado. `reintentar` no alcanza ahi: yt-dlp saltea
  un archivo que existe y volveria a decir `listo` sobre el mismo roto.
- **reintentar**: mismos argumentos. Si quedo un `.part`, yt-dlp
  **reanuda**. En un trabajo ya listo, termina rapido porque el archivo
  esta.

Si hay varios trabajos terminados, arriba aparece `reintentar todo (N)`.

Al vaciarse la cola (de tener trabajo a no tener), reel avisa una vez
con `notify-send`, no una vez por archivo.

## Si falla

La fila dice `fallo` y el error de yt-dlp tal cual. En los que se
repiten, tambien un consejo: un `403` de YouTube sugiere esperar; un
video privado, las cookies; un formato que no existe, elegir otro.

No hay captura de un fallo en estas guias: el video de prueba termino
bien.
