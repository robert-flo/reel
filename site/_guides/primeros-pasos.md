---
title: Primeros pasos
description: Pegar un enlace, buscarlo y mandarlo a bajar.
nav_order: 1
---

# Primeros pasos

Pegas un enlace, apretas `buscar`, elegis el formato, y `descargar` lo
manda a la cola. Eso es todo el gesto.

## La ventana vacia

Al abrir reel ves la barra de arriba (`reel`, la version, `ajustes`), el
campo `pega un enlace`, los botones `pegar` y `buscar`, y la cola vacia.

![Pantalla inicial con la cola vacia]({{ '/images/pantalla-inicial.png' | relative_url }})

El pie dice donde van a caer los archivos. Si no tocaste los ajustes,
el video va a `~/Videos` y el audio a `~/Music`. A la derecha, el tema
y las versiones de `yt-dlp` y `ffmpeg`. Si falta alguna de las dos, el
pie lo avisa en ambar.

## Pegar el enlace

`pegar` lee el portapapeles (`wl-paste` en Wayland) y deja el enlace en
el campo. Tambien podes escribirlo o pegarlo con el teclado. Con algo
escrito, `buscar` se enciende.

![Enlace de archive.org pegado en el campo]({{ '/images/enlace-pegado.png' | relative_url }})

En estas capturas el enlace es el video corto de prueba de archive.org.
Vale cualquier sitio que sepa yt-dlp.

## Buscar

`buscar` (o Enter en el campo) **no baja nada**. Lee el enlace: titulo,
autor, duracion, de donde viene. Mientras tanto el boton dice `leyendo`
y no se puede apretar de nuevo.

![El boton buscar dice leyendo]({{ '/images/leyendo-enlace.png' | relative_url }})

Cuando termina, aparece la ficha. Ahi elegis el formato y recien ahi
mandas a descargar. Sigue en [buscar]({% link _guides/buscar.md %}).
