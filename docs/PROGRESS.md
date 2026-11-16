# Progreso

Bitácora del proyecto. Se actualiza al terminar cada paso.

## 2026-11-15 — Versión inicial (0.1.0)

### Hecho

| Paso | Estado | Notas |
|---|---|---|
| Toolchain | ✅ | Rust 1.100, target wasm32, trunk, tauri-cli. |
| `protocol` | ✅ | Tipos compartidos + tests de formato de protocolo. |
| `core` · métricas | ✅ | `sysinfo` (CPU, RAM, swap, red, discos, procesos). |
| `core` · batería | ✅ | `ioreg -a` + `plist`. Verificado contra `ioreg` en un Mac real (ciclos, salud, temperatura). |
| `core` · SMC | ✅ | FFI directo a IOKit. Enumera las ~3400 llaves del SMC. **Enteros en little-endian en Apple Silicon** (descubierto y corregido). |
| `core` · sensores | ✅ | Ventiladores (RPM, mín/máx, modo) y ~80 sensores de temperatura. |
| `core` · almacenamiento | ✅ | Uso por carpeta, archivos grandes, duplicados, 10 categorías, borrado con guardas + token. |
| `helper` | ✅ compila | Socket con `getpeereid`, comandos cerrados, detección de llaves escribibles, seguridad térmica. |
| `app` (Tauri) | ✅ | 24 comandos, evento `metrics`, ícono en barra de menú, ícono de app. |
| `frontend` (Leptos) | ✅ | 8 páginas, componentes propios, modo de datos simulados. Hecho por un subagente en paralelo. |
| Docs | ✅ | README, ARCHITECTURE, DESIGN, CLEANUP_RULES, HELPER_SECURITY, SMC_KEYS, DEVELOPMENT, CHANGELOG. |

### Verificado

- `cargo test`: 11 tests OK (guardas de borrado, escape por symlink, token de un uso, duplicados, decodificación SMC, parseo de batería).
- `trunk build` sin errores; UI revisada con datos simulados en el navegador.
- `cargo tauri dev` arranca la app real.
- `iclear-cli categories` sobre un Mac real (solo lectura): tamaños coherentes con `du -sh`.

### Errores encontrados y corregidos

1. **Endianness SMC**: `ui16/ui32` venían en LE; los valores salían absurdos (`CHA1` = 671 940 608 en vez de 3368).
2. **Tamaños en 0 B**: un recorrido `jwalk` anidado dentro de un pool de rayon ocupado se rendía por timeout y devolvía vacío. Ahora `path_size` recorre en serie (ya se paraleliza por ítem).

## 2026-11-15 — Helper instalado y probado

### Hecho
- **Bug de instalación corregido**: el script root no podía leer el binario desde `~/Desktop` (protección TCC). Ahora la app lo copia antes a una carpeta temporal privada (`0700`).
- **Bug de lectura corregido**: `F{i}Md = 3` significa automático con el ventilador detenido; antes se mostraba como "Manual".
- `iclear-cli helper send '<json>'` para probar el helper desde la terminal.

### Verificado en el Mac real
| Prueba | Resultado |
|---|---|
| Estado del helper | instalado, responde; socket `0600` del usuario instalador |
| Detección | ventiladores ✅ · descarga forzada (`CHIE`) ✅ · límite de carga ❌ (macOS 27) |
| Ventilador 0 → manual 4000 rpm | ✅ alcanza ~4000 rpm en ~6 s |
| Ventilador 0 → automático | ✅ el sistema retoma el control (0 rpm en reposo) |
| Vaciar caché DNS | ✅ |
| Ventilador inexistente / límite 5 % / límite en macOS 27 / comando desconocido | ✅ rechazados con mensaje claro |

## 2026-11-15 — Memoria inteligente y apps sin uso

### Hecho
- `core/memory.rs`: desglose, `phys_footprint`, agrupación por proceso responsable, registro de foco (`focus.json`), muestras de crecimiento y 12 reglas de recomendación. Detalle: [INTELLIGENCE.md](INTELLIGENCE.md).
- `core/apps.rs`: escaneo de `/Applications` y `~/Applications` con veredicto de uso, asignación de restos sin ambigüedades, huérfanos conservadores y vistas previas de desinstalación.
- `Guard::check_app_removal` y vistas previas marcadas como app_removal.
- 5 comandos Tauri nuevos; el hilo de métricas llama a `MemoryIntel::tick()` cada ~6 s.
- UI de Memoria y Apps: la hace un subagente en paralelo.

### Verificado en el Mac real (sin borrar nada)
- `iclear-cli memory`: el desglose coincide con el Monitor de Actividad; detecta la presión, el swap, las pestañas de los navegadores asignadas a su app y las máquinas virtuales (Docker/Podman) asignadas a la app que las lanzó.
- `iclear-cli apps`: escanea decenas de apps en ~6 s y encuentra restos huérfanos de apps ya desinstaladas.
- `apps preview`: arma la lista del paquete y sus restos; rechaza apps abiertas. No se borró nada.
- **Bug evitado**: un pequeño "URL handler" se quedaba con los datos de la app principal porque compartían nombre de ejecutable. Ahora no se compara por ejecutable, una carpeta ambigua no se asigna a nadie y hay un test para esto.
- `cargo test`: 19 tests OK.

## 2026-11-15 — Instalador, repo y privacidad

### Hecho
- `scripts/build-pkg.sh` genera `dist/iclear-0.1.0.pkg` (5,2 MB): trae la app con el helper dentro, firma ad-hoc, instala en /Applications y exige arm64 con macOS 13 o superior.
- Repo publicado en GitHub: `iveas24/iclear`, rama `main`.
- Los docs, los datos simulados y los tests ya no contienen datos del Mac de desarrollo.
- Sección Nube agregada por otra sesión en paralelo. Ver [CLOUD.md](CLOUD.md).

### Pendiente / por verificar

- [x] Instalar el helper y probar ventilador manual → auto.
- [ ] Probar la descarga forzada (`CHIE`) con la batería sobre 50 % (desconecta el cargador mientras está activa).
- [ ] Revisar la app real con datos reales (no pude capturar la ventana nativa: falta permiso de grabación de pantalla).
- [ ] Dar **Acceso total al disco** a la app para medir Mail y contenedores.
- [x] Empaquetar `iclear-helper` dentro del `.app` y generar el instalador `.pkg` (`scripts/build-pkg.sh`).
- [ ] Desinstalar apps de root (App Store / `.pkg`): hoy salen como no removibles. Se podría ofrecer "Mover a la Papelera" con el permiso de Finder o un comando del helper con validación estricta.
- [ ] Publicar el `.pkg` como Release de GitHub (requiere `gh auth login`).
- [ ] Firmar y notarizar con una Developer ID para evitar el aviso de Gatekeeper.
- [ ] Probar una desinstalación real (con tu OK) y revisar `deletions.log`.
- [ ] Opcional: permitir Google Fonts en la CSP o empaquetar las fuentes (hoy hay respaldo a SF Pro / SF Mono).
