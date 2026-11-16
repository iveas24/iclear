# Diseño de la interfaz de iclear

Interfaz oscura, minimalista y “futurista contenida”, inspirada en metalforge.xyz: fondo casi negro, tonos **plomo** y acentos **verdes** suaves con un leve brillo. Todo el texto visible está en español de Chile (coma decimal, punto como separador de miles).

## Stack

- **Leptos 0.8 (CSR)** compilado a WASM con **Trunk** (`frontend/`).
- Sin router: la navegación es una señal (`AppState.page`) controlada por la barra lateral.
- Puente con Tauri en `frontend/src/api.rs` (`window.__TAURI__.core.invoke` y `window.__TAURI__.event.listen`).
- Tipos compartidos desde `iclear-protocol` (`backend/protocol`).

```
frontend/
├── index.html          # carga fuentes de Google y los CSS vía Trunk
├── Trunk.toml          # dist = dist, servidor en 127.0.0.1:1420
├── styles/tokens.css   # tokens de diseño (solo modo oscuro)
├── styles/app.css      # layout + componentes
└── src/
    ├── main.rs         # monta <App/>
    ├── app.rs          # shell: barra lateral, barra superior, toasts
    ├── state.rs        # AppState (contexto global), historial de métricas, toasts
    ├── api.rs          # wrappers tipados de cada comando + evento `metrics`
    ├── mock.rs         # backend simulado para vista previa en navegador
    ├── fmt.rs          # formato es-CL (bytes, °C, rpm, tiempos…)
    ├── components/     # piezas reutilizables
    └── pages/          # una página por sección
```

## Tokens (`styles/tokens.css`)

| Token | Valor | Uso |
|---|---|---|
| `--bg` | `#0b0c0d` | Fondo de la app |
| `--surface` | `#141617` | Tarjetas |
| `--surface-2` / `--surface-3` | `#1c1f20` / `#232728` | Controles, estados activos |
| `--hairline` / `--hairline-strong` | `#ffffff14` / `#ffffff24` | Bordes de 1 px |
| `--dot` | `#ffffff0d` | Retícula de puntos del fondo |
| `--text` / `--text-2` / `--text-3` | `#eceeed` / `#8d9392` / `#5c6261` | Texto principal, secundario, terciario |
| `--accent` | `#3ddc84` | Verde de acento (anillos, botones primarios, foco) |
| `--accent-soft` / `--accent-glow` | `#3ddc841f` / `#3ddc8466` | Fondos de insignias, brillos |
| `--warn` / `--danger` | `#e5b454` / `#ef5b5b` | Advertencias, borrado |
| `--shadow-soft` | `0 1px 0 #ffffff0a inset, 0 24px 60px -24px #0009` | Elevación de tarjetas |
| `--radius` / `--radius-lg` | `14px` / `20px` | Radios; botones y chips son píldoras (`999px`) |
| `--t-fast` / `--t-med` | `150ms` / `220ms` | Transiciones |

El fondo lleva una retícula de puntos (`radial-gradient` con `--dot`) enmascarada hacia abajo y un halo verde muy tenue arriba.

## Tipografía

- **Geist** para títulos (`--font-display`), tracking `-0.02em`.
- **Inter** para el cuerpo (`--font-body`), 14 px.
- **Geist Mono** con `tabular-nums` para todas las cifras (`.num`, `.mono`), así los valores en vivo no “bailan”.
- Respaldo: `-apple-system`, SF Pro / SF Mono. Las fuentes se cargan desde Google Fonts; sin conexión se usan las del sistema.
- Etiquetas de sección (`.eyebrow`, `.nav-label`): 10,5 px, mayúsculas, tracking `0.08em`, color `--text-3`.

## Estructura de la ventana

- **Barra lateral fija** (232 px; 224 px bajo 1080 px). Cabecera de 52 px con `data-tauri-drag-region` y `padding-left: 78px` para los semáforos de macOS (barra de título transparente). Secciones *Monitor* (Panel, Memoria, Batería, Ventiladores), *Disco* (Almacenamiento, Limpieza, Apps), *Sistema* (Procesos, Optimizar); cada ítem muestra un dato en vivo a la derecha (CPU %, memoria %, batería %, rpm, espacio libre, nº de apps sin uso cuando ya hay análisis). El ítem activo lleva una barra verde con brillo. Al pie: Ajustes y una tarjeta del equipo con un punto de estado (verde = asistente activo).
- **Barra superior** (64 px, también arrastrable): sección + título de página, insignia “Vista previa · datos simulados” en modo mock, indicador pulsante “En vivo” y chip de estado del asistente (lleva a Ajustes).
- **Contenido**: máx. 1320 px, rejilla de tarjetas con separación de 16 px, animación de entrada de 220 ms. El scroll vuelve arriba al cambiar de página.

## Componentes (`src/components/`)

| Componente | Archivo | Notas |
|---|---|---|
| `Card`, `StatTile`, `InfoRow` | `card.rs` | Tarjeta con borde fino; métrica compacta (etiqueta, cifra mono, unidad, subtítulo, tono). |
| `Ring` | `ring.rs` | Anillo SVG 0–100 con pista de marcas, trazo verde con `drop-shadow`; tonos `accent`/`warn`/`danger` (`load_tone`). |
| `Sparkline` | `sparkline.rs` | Línea + área con degradado verde, se llena desde la derecha, punto “vivo” al final; escala fija o automática. |
| `Pill`, `Chip`, `Badge`, `Toggle`, `Check`, `Slider`, `Segmented`, `SearchField` | `controls.rs` | Botones píldora (`primary`, `ghost`, `danger`, `sm`) con spinner; chips de filtro; insignias con punto brillante; interruptor; casilla; slider con relleno vía `--fill`; control segmentado. |
| `SortHeader`, `Meter` | `table.rs` | Encabezado ordenable (`aria-sort`); barra horizontal delgada. |
| `ConfirmDialog`, `CleanupReview` | `dialog.rs` | Modal (Esc/clic fuera cierra). `CleanupReview` muestra el `CleanupPreview`, permite desmarcar ítems, avisa “Borrado permanente — no se puede deshacer” y llama `execute_cleanup { token, paths }`; acepta un `title` opcional. Se usa en Limpieza, Almacenamiento y Apps (desinstalar / restos huérfanos). |
| `ToastHost` | `toast.rs` | Avisos abajo a la derecha (éxito/error/info), se cierran solos. |
| `HelperRequired`, `HelperCard`, `UnsupportedNote` | `helper.rs` | Tarjeta tranquila que ofrece instalar el asistente; nota cuando el hardware/macOS no permite la función. |
| `Loading`, `Skeleton`, `Spinner`, `EmptyState`, `ErrorNote` | `feedback.rs` | Estados de carga (los escaneos tardan segundos), vacíos y errores. |
| `Tip` | `feedback.rs` | Tooltip propio (hover y foco, `role="tooltip"`), burbuja `--surface-3` con borde fino; `side="left"` lo alinea a la derecha. Envuelve también botones deshabilitados (el `title` nativo no aparece en ellos). |
| `Scanning` | `feedback.rs` | Estado de análisis lento: “radar” circular con barrido cónico verde, ícono al centro y pasos que rotan cada 1,1 s (“Midiendo el tamaño de cada app…”). |
| `Icon`, `Logo` | `icons.rs` | Íconos de trazo en línea (24×24, 1,6 px). Sin fuentes de íconos. |

## Páginas (`src/pages/`)

- **Panel** — 4 anillos (CPU, memoria, disco “/”, batería; cada uno lleva a su sección), gráfico de CPU de los últimos ~90 s (60 muestras) con barras por núcleo, gráfico de temperatura de CPU con máx/GPU/SSD, ventiladores, red (↓/↑ con mini gráficos) y datos del equipo.
- **Memoria** — explica *por qué* el Mac usa tanta memoria (`get_memory_report`, cada 5 s mientras la página está visible; el último informe queda en `AppState.memory`). Héroe: anillo de salud (`score`, verde ≥ 70, ámbar ≥ 40, rojo), insignia de presión (Normal / Advertencia / Crítica) y un diagnóstico en una frase (“Tus apps piden 31 GB en un Mac de 24 GB: macOS está comprimiendo 19,7 GB en 9,1 GB y usando 6,9 GB de swap”, donde demanda = `app + compressed_original`), más swap usado/total con barra, “comprimido X → Y” con relación y % disponible. Barra apilada *En qué se va la RAM*: App (verde), Sistema (fija) (plomo claro), Comprimida (ámbar), Caché de archivos (verde rayado), Libre; leyenda con `Tip` que explica cada grupo (la caché se libera al instante; comprimida y swap son el síntoma). *Recomendaciones inteligentes*: `Insight` ordenados por severidad (Crítica rojo, Advertencia ámbar, Sugerencia verde, Info neutro), insignia “libera ~2,1 GB” y acción: `QuitApp` → confirmación → `quit_app { mainPid }`; `Optimize` → `run_optimization` (sin asistente lleva a Ajustes); `OpenPage` → navega (`Page::from_id`). *Qué está usando tu memoria*: tabla ordenable de `AppMemory` (nombre + chip de tipo, procesos y “N pestañas/renderers”, huella con barra proporcional, % de RAM, CPU, “Sin usar hace 2 h” solo para apps con ventana, “+350 MB/h” en ámbar si crece > 200 MB/h, botón Cerrar si `killable` y hay `main_pid`); 25 filas con “Ver todas”. Nunca se ofrece cerrar Claude (`com.anthropic.claudefordesktop`), ni en la tabla ni en las recomendaciones. Si `observed_secs < 120` aparece la nota “iclear aprende tus hábitos…”.
- **Batería** — anillo grande con %, estado (cargando / carga completa / en pausa por límite / usando batería), tiempo restante, adaptador; mosaico de salud, ciclos, temperatura, voltaje, potencia y tiempo; capacidad actual/máxima/diseño. *Control de carga*: interruptor + slider 20–100 (paso 5) → `set_charge_limit`, y descarga forzada → `set_force_discharge`. Sin asistente: `HelperCard`. Si `supports_charge_limit`/`supports_force_discharge` son `false` (macOS 27): controles deshabilitados y nota con botón “Abrir ajustes de batería” → `open_battery_settings`.
- **Ventiladores** — insignia “Experimental”; tarjeta por ventilador con anillo de rpm entre mín. y máx., Auto/Manual (`set_fan_auto` / `set_fan_rpm`), slider de rpm (en Auto refleja la velocidad real). Nota si `supports_fan_control` es `false`. Temperaturas: resumen (CPU prom./máx., GPU, batería, SSD) y tarjetas por `group` (CPU, GPU, Memoria, Batería, SSD, Wi‑Fi, Chasis, Energía, Ambiente) en columnas tipo mampostería; la etiqueta `"CPU P-core · Tp01"` se separa en nombre y clave.
- **Almacenamiento** — anillo del disco “/” con leyenda; desglose de carpetas (`disk_usage`) con barras horizontales, clic para entrar, migas de pan para volver, “Mostrar en Finder”. Pestañas *Archivos grandes* (`large_files`, mínimo 50 MB–1 GB) y *Duplicados* (`find_duplicates`, botón “Marcar todas las copias”). La selección aparece en una barra flotante → `preview_paths` → `CleanupReview`.
- **Limpieza** — resumen (recuperable, seleccionado, categorías) + rejilla de categorías con riesgo Seguro/Moderado/Revisar, preseleccionadas según `selected_by_default`; las que requieren asistente se bloquean sin él. “Revisar y limpiar” → `preview_cleanup` → `CleanupReview` → toast con lo liberado y errores → nuevo análisis.
- **Apps** (“Apps sin uso”) — `get_apps_report` tarda varios segundos: se muestra `Scanning` + esqueletos y el resultado queda en `AppState.apps` hasta pulsar “Volver a analizar” (al volver a analizar, la lista anterior se atenúa). Mosaicos: apps instaladas, sin uso, espacio recuperable (`unused_bytes`), restos huérfanos (`orphans_bytes`). Chips de filtro con conteo (Todas / Sin uso / Ocasionales / Activas / Se abren al iniciar) y orden segmentado (Tamaño / Último uso / Nombre). Cada fila: inicial en un cuadro, nombre, versión, insignia de veredicto (Activa verde, Ocasional neutra, Sin uso ámbar, Sin datos), motivo, etiquetas (En ejecución, Al iniciar, Electron, App Store), tamaño total con “app + restos”, último uso (“hace 4 meses”, “nunca”, “sin datos”); el chevron despliega la app y sus restos (tipo, ruta con `~`, tamaño). “Desinstalar” → `preview_uninstall { appPath }` → `CleanupReview` → `execute_cleanup` → toast con lo liberado → nuevo análisis; si `removable` es `false`, el botón queda deshabilitado con un `Tip` que explica por qué. *Restos de apps ya desinstaladas*: lista de `OrphanGroup` por `bundle_id` con casilla (todos preseleccionados), antigüedad y tamaño; “Eliminar seleccionados” → `preview_orphans { bundleIds }` → `CleanupReview`.
- **Procesos** — tabla ordenable (nombre, PID, CPU con barra, memoria, usuario), búsqueda, actualización cada 3 s (desactivable), Terminar/Forzar solo si `killable`, con confirmación (SIGTERM vs. SIGKILL).
- **Optimizar** — tarjetas para las 5 `OptimizeAction` (confirmación para snapshots, Spotlight y cachés del sistema); requieren asistente. *Elementos de inicio*: agentes de usuario y globales con interruptor (`set_launch_agent_enabled`), daemons solo lectura.
- **Ajustes** — estado del asistente (instalado, conectado, versión, protocolo, capacidades) con Instalar/Reinstalar/Desinstalar; explicación de Acceso total al disco + botón `open_full_disk_access_settings`; datos del Mac; Acerca de (sin enlaces).

## Accesibilidad y movimiento

- Anillo de foco verde (`:focus-visible`), interruptores con `role="switch"`, casillas con `role="checkbox"`, pestañas con `role="tab"`, `aria-sort` en tablas, `aria-live` en toasts.
- Contraste: texto principal `#eceeed` y secundario `#8d9392` sobre `#141617`.
- `prefers-reduced-motion`: se anulan animaciones y transiciones (incluido el pulso “en vivo”).
- Responsive: probado a ~1000 px y ~800 px (Memoria y Apps); las rejillas de 4/3 columnas pasan a 2, el disco y el desglose se apilan, se ocultan columnas secundarias de las tablas.

## Vista previa con datos simulados

Fuera de Tauri (`window.__TAURI__` no existe) cada comando responde con datos de `src/mock.rs`, que simula un MacBook Pro genérico con Apple Silicon: batería 35 % cargando, 212 ciclos, salud 93 %, 2 ventiladores (1200–6000 rpm), ~20 sensores de temperatura, 12 núcleos, 24 GB de RAM con presión en Advertencia (9,1 GB comprimidos que guardan 19,7 GB, swap 5,4/8 GB; Chrome 6,2 GB con 38 pestañas, Docker 4 GB, Claude, Slack con fuga, WindowServer…), disco de 460 GB, 18 apps instaladas (Dropbox, Teams, Steam, Acrobat sin uso; GarageBand/iMovie de Apple no desinstalables) y 5 grupos de restos huérfanos (p. ej. `com.evernote.evernote`). Los datos simulados son ficticios: no reflejan ningún Mac real. El evento `metrics` se emula con un temporizador de 1,5 s y el historial de gráficos se prellena. El simulador mantiene estado: el asistente empieza **sin instalar** (se ve la tarjeta de instalación) y “Instalar asistente” lo activa; `supports_charge_limit = false` y `supports_fan_control = true` para mostrar ambos estados; los borrados quitan archivos/categorías, apps desinstaladas y restos en análisis posteriores; los procesos terminados desaparecen; cerrar una app en Memoria la quita del informe y alivia compresión, swap y presión. El “tiempo observado” cuenta desde que se cargó la página, así que la nota de aprendizaje y el crecimiento por hora aparecen tras ~2 min.

```sh
source ~/.cargo/env
cd frontend
trunk serve --port 1420      # abre http://localhost:1420
trunk build                  # genera frontend/dist (lo usa Tauri: frontendDist ../../frontend/dist)
```

En la app real, Tauri usa `devUrl: http://localhost:1420` y `withGlobalTauri: true`, por lo que el mismo código usa los comandos reales automáticamente.
