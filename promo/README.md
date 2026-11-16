# Video promocional (historia de Instagram)

`iclear-story.mp4`: 1080×1920, 30 fps, **44,4 s** y 10 escenas. Lleva voz en off masculina estilo tráiler de película y música original, mezcladas a −14 LUFS.

> **Sin datos reales.** Todas las cifras son de un Mac ficticio de demostración: 16 GB de RAM, Chrome con 42 pestañas, Docker, Figma, Spotify, etc. Las capturas de la app se toman con datos simulados, y antes de capturar se reemplazan los textos que se parecían al Mac de desarrollo (`DEMO_TEXT` en `shots.mjs`).

## Cómo se arma

| Archivo | Qué hace |
|---|---|
| `timeline.js` | **Fuente única de verdad**: orden de las escenas, duración de cada una (múltiplos del pulso de 0,6 s, así cada corte cae en un bombo), acorde y frase de voz. |
| `story.html` | Animación en canvas. Si lo abres en el navegador, se reproduce en bucle. |
| `shots.mjs` | Captura las pantallas Panel, Memoria y Ventiladores de la app (frontend en `:1420`, con datos simulados) y genera `shots.js`. |
| `audio.mjs` | Música sintetizada desde cero (`music.wav`). Tiene un acorde por escena, bombo, palmas, hi-hats, bajo y arpegio, más efectos sincronizados: drop, whooshes, pops, clics, chimes e impacto final. |
| `voice.mjs` | Voz en off con `say` (`voice.wav`). Por defecto es el estilo **tráiler**: voz masculina Eddy (es_MX), 3 semitonos más grave, graves de pecho, compresión de locutor, reflexiones cortas y un ensanchador estéreo (efecto Haas) para que suene envolvente. Si una frase no cabe en su escena, el script falla y dice cuál. |
| `render.mjs` | Chrome headless captura los cuadros y ffmpeg los codifica. La música baja sola cuando habla la voz (sidechain) y el volumen se normaliza. |
| `cdp.mjs`, `timeline.mjs` | Utilidades compartidas: cliente DevTools y lector del timeline. |

```bash
node promo/shots.mjs      # solo si cambió la UI (necesita el frontend en :1420)
node promo/audio.mjs && node promo/voice.mjs && node promo/render.mjs
```

## Guion

| # | Escena | Imagen | Voz |
|---|---|---|---|
| 1 | Gancho | "¿Tu Mac va lento?" y una barra donde las apps piden 23,5 GB en un Mac de 16 GB | Tu Mac no está viejo. Está saturado. |
| 2 | Presentación | Ventana del Mac que entra volando y recorre pantallas reales de la app; chips con las funciones | Te presento iclear: el centro de control que tu Mac necesitaba. |
| 3 | Memoria | Puntaje de salud y las apps que más gastan | Detecta en segundos qué está devorando tu memoria, y por qué. |
| 4 | Recomendaciones | Tarjetas; se cierra Spotify y aparece "Hasta 6,6 GB de vuelta" | Recomendaciones inteligentes: recupera gigas con un solo toque. |
| 5 | Apps sin uso | 9,8 GB; se desinstala GarageBand sin restos | Elimina las apps que ya no usas, sin dejar ni un archivo atrás. |
| 6 | Limpieza | Se marcan las categorías, aparecen 7,4 GB y "Revisar y limpiar" | Limpia cachés y basura, siempre con vista previa. |
| 7 | Control | Batería, ventilador girando, temperaturas y procesos | Batería, ventiladores y temperatura, bajo control. |
| 8 | Optimizar | Barra de menú con iclear y 4 acciones que se completan | Optimiza con un clic, desde tu barra de menú. |
| 9 | Privacidad | Escudo; "Tus datos no salen de tu Mac" | Todo es local: tus datos nunca salen de tu Mac. |
| 10 | Cierre | Logo, "Tu Mac, como nuevo.", "Pronto para macOS" y "Síguenos" | iclear. Tu Mac, como nuevo. Síguenos para el lanzamiento. |

## Dinamismo

- **Cámara:** cada escena entra con un golpe de zoom y sale empujada hacia arriba.
- **Pulso:** todo el contenido late con cada bombo y el fondo se ilumina al mismo ritmo.
- **Cortes:** destello y barrido de luz verde en cada cambio de escena.
- **Texto:** los títulos entran palabra por palabra con rebote y las etiquetas se escriben como en una terminal.
- **Fondo:** partículas que suben y un brillo que se mueve.
- **Remate:** sacudida en el impacto del logo y una flecha que rebota hacia el botón de seguir.

## Para cambiar

- **Una frase o la duración de una escena:** se edita `timeline.js` y se corre de nuevo `audio.mjs`, `voice.mjs` y `render.mjs`.
- **Otros estilos de voz:** `STYLE=social` (voz femenina tipo TikTok/Reels) o `STYLE=natural` (narrador tranquilo), por ejemplo `STYLE=social node promo/voice.mjs`.
- **Otra voz:** `VOICE="Reed (Español (México))" node promo/voice.mjs`; la lista está en `say -v '?'`. Las voces "Mejorada" o "Premium" suenan más naturales; se descargan en Ajustes › Accesibilidad › Contenido leído.
- **Pronunciación:** en `timeline.js` la marca está escrita "Ai clir" para que la voz en español diga "iclear" como en "iPhone".
