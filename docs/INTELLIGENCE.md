# Inteligencia: memoria y apps sin uso

Cómo iclear explica en qué se va la memoria y detecta lo que no usas. Código: `backend/core/src/memory.rs` y `backend/core/src/apps.rs`.

## Memoria

### Desglose (igual que el Monitor de Actividad)

Viene de `vm_stat` y `sysctl`:

| Balde | Cálculo | Qué significa |
|---|---|---|
| Apps | `Anonymous pages − Pages purgeable` | Lo que piden las apps |
| Fija (wired) | `Pages wired down` | Kernel y drivers; no se puede liberar |
| Comprimida | `Pages occupied by compressor` | RAM que usa el compresor |
| Comprimido original | `Pages stored in compressor` | Cuánto ocuparía eso sin comprimir |
| Caché de archivos | `File-backed pages + purgeable` | Se libera al instante; no es un problema |
| Libre | `Pages free + speculative` | — |
| Presión | `kern.memorystatus_vm_pressure_level` | 1 = normal · 2 = advertencia · 4 = crítica |
| Nivel libre | `kern.memorystatus_level` | % que el kernel considera disponible |
| Swap | `vm.swapusage` | Lo que se guardó en el SSD |

**Demanda real** = apps + fija + comprimido original. Si supera la RAM, el Mac comprime y usa swap: ese es el síntoma de "consume mucha memoria".

**Puntaje (0–100)**: parte de `kern.memorystatus_level`, resta 3 puntos por cada GB de swap (máximo 30), tiene un piso de 60 con presión normal y un techo de 59 con advertencia o de 25 con presión crítica.

### Agrupar procesos por app

1. Por cada proceso se mide `phys_footprint` (`proc_pid_rusage`, `RUSAGE_INFO_V4`), la misma cifra que el Monitor de Actividad llama "Memoria". Si el proceso es de otro usuario, se usa el RSS.
2. Se busca el **proceso responsable** con `responsibility_get_pid_responsible_for_pid`, una API privada de libSystem que usa el Monitor de Actividad. Así:
   - los renderers de Chrome cuentan para Chrome;
   - los `WebKit.WebContent` cuentan para Safari;
   - la `Virtualization.VirtualMachine` cuenta para Docker, Podman o Nod.
3. La app es el `X.app` más externo de la ruta del responsable.
4. Tipo (`AppKind`):
   - **navegador**, **desarrollo** y **virtualización** se reconocen por el bundle id;
   - **Electron**, si el bundle trae `Electron Framework.framework`;
   - **segundo plano**, si el `Info.plist` tiene `LSUIElement` o `LSBackgroundOnly`;
   - **sistema**, si vive en `/System/Library`.

   Un grupo que contiene `Virtualization.VirtualMachine`, `vfkit`, `krunkit` o `qemu-system` pasa a ser de virtualización.
5. Los procesos sueltos del sistema de menos de 80 MB se juntan en "Servicios de macOS".

### Aprende tus hábitos

- Cada ~6 s se anota qué app está al frente (`lsappinfo front`) y se guarda en `~/Library/Application Support/iclear/focus.json`. Este registro sobrevive a reinicios y alimenta el detector de apps sin uso.
- Cada 30 s se toma una muestra del footprint de cada app y se conservan 2 horas. Con al menos 10 minutos de muestras se calcula el crecimiento en bytes por hora para detectar fugas.

### Recomendaciones (`insights`)

| Id | Cuándo | Acción |
|---|---|---|
| `pressure` | Siempre (diagnóstico en lenguaje simple; va primero) | — |
| `idle:<app>` | App con ventana de ≥ 300 MB que no está al frente y cumple una de estas: (a) sin foco hace ≥ 30 min; (b) iclear la observa hace ≥ 30 min y nunca la vio al frente; (c) lleva ≥ 6 h abierta con CPU < 0,5 % | Cerrar app |
| `browser:<app>` | Navegador con ≥ 15 renderers o ≥ 2 GB | Consejo según el navegador (Ahorro de memoria, pestañas en reposo…) |
| `electron` | ≥ 2 apps Electron de ≥ 150 MB | — |
| `vm:<app>` | Virtualización ≥ 1 GB | Cerrar app |
| `leak:<app>` | ≥ 400 MB y crece ≥ 300 MB/h | Cerrar app |
| `background` | Apps de segundo plano que suman ≥ 600 MB | Ir a Optimizar |
| `spotlight` | `mds*` / `mdworker*` ≥ 500 MB | Ir a Optimizar |
| `windowserver` | WindowServer ≥ 1,5 GB | — |
| `uptime` | ≥ 7 días encendido y con presión | — |
| `purge` | Con presión y ≥ 1 GB de caché (se explica que el efecto es temporal) | `PurgeMemory` |
| `apps` | Con presión | Ir a Apps |

Nunca se sugiere cerrar la app que está al frente ni iclear. `quit_app` se niega a cerrar Claude (cortaría la sesión), cierra con AppleScript `quit` por bundle id (así la app puede pedir guardar) y solo actúa sobre procesos del usuario.

### Ejemplo (Mac ficticio de 16 GB)

Las apps y el sistema piden 23,5 GB; macOS comprime 12,4 GB en 5,8 GB y usa 3,2 GB de swap, así que el puntaje queda en 31/100 con presión en advertencia. Las recomendaciones apuntan a Chrome (42 pestañas, 5,8 GB), a la VM de Docker (3,1 GB) y a Spotify abierta sin uso hace 6 horas.

## Apps sin uso

### Señales

| Señal | Fuente | Peso |
|---|---|---|
| Última apertura | Spotlight `kMDItemLastUsedDate` (lo actualiza LaunchServices) | Fuerte |
| Última vez al frente | `focus.json` de iclear | Fuerte |
| Abierta ahora | Procesos en ejecución | Definitiva |
| Datos modificados | mtime más reciente de sus carpetas en `~/Library` (sin contar Registros) | Débil: también escriben ahí los agentes en segundo plano |
| Fecha de llegada | `kMDItemDateAdded` | Débil: se reinicia con cada actualización |

### Veredicto

- **Activa**: abierta ahora o usada en los últimos 30 días.
- **Ocasional**: usada hace 30–90 días, o sin registro de uso pero con datos modificados en los últimos 90 días.
- **Sin uso**: más de 90 días sin abrirse; o sin registro de uso y datos sin cambios hace más de 90 días; o sin ningún dato y llegó hace 30 días o más.
- **Sin datos**: el resto. Típico de apps recién instaladas o actualizadas.

### Restos de una app instalada

Son los hijos directos de estas carpetas de `~/Library`: `Application Support`, `Caches`, `Containers`, `Preferences`, `Saved Application State`, `HTTPStorages`, `WebKit`, `Application Scripts`, `Logs` y `LaunchAgents`.

- **Coincidencia por bundle id**: el nombre es igual al id o empieza con `id.`. Esa carpeta es solo para la app con ese id.
- **Coincidencia por nombre**: el nombre es igual al de la app o al del archivo `.app`, con al menos 4 letras y sin nombres genéricos. Se le asigna solo si **ninguna otra app** reclama esa carpeta.
- Una carpeta ambigua no se ofrece nunca. Ejemplo: un pequeño "URL Handler" de una app compartía el nombre de carpeta con la app principal; ahora sus datos solo van con la app principal (test `ambiguous_leftovers_are_not_assigned`).
- Si hay dos copias de la app con el mismo bundle id, desinstalar una borra solo el paquete.

### Restos huérfanos (apps ya desinstaladas)

Una carpeta de datos cuenta como huérfana si cumple todo esto:
- su nombre tiene forma de bundle id (`tld.vendor.producto`);
- no es de Apple (`com.apple.*`, `group.*`);
- ninguna app que conozca Spotlight tiene ese id ni comparte su prefijo;
- **ningún** app instalada es del mismo vendor. Por ejemplo, `com.google.Keystone` no se toca si está instalado Chrome;
- ningún LaunchAgent usa ese prefijo;
- no se ha modificado en los últimos 30 días.


### Desinstalar

`preview_uninstall` y `preview_orphans` arman la lista en el backend: nunca confían en rutas enviadas por la UI. Además usan una guarda distinta y más estricta, `Guard::check_app_removal`. Ver [CLEANUP_RULES.md](CLEANUP_RULES.md).

Una app no es removible en estos casos:
- está abierta;
- es Safari o iclear;
- el paquete o su carpeta no son escribibles por tu usuario. Es lo típico de apps del App Store o instaladas con `.pkg`, que son de root.
