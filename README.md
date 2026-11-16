# iclear

Panel de control y optimizador para Mac, escrito 100 % en Rust: **backend** (Tauri 2 + daemon root) y **frontend** (Leptos → WASM). Interfaz oscura y minimalista en tonos plomo con acentos verdes.

## Qué hace

| Área | Funciones |
|---|---|
| **Memoria** | Explica por qué el Mac usa tanta memoria: desglose como el Monitor de Actividad, procesos agrupados por la app responsable (pestañas, renderers, VMs), apps abiertas sin uso, fugas y recomendaciones con cuánto liberan. |
| **Apps** | Detecta apps que no usas (Spotlight + registro propio de qué app tienes al frente), su peso con los datos que dejan en `~/Library`, desinstalación completa y restos de apps ya desinstaladas. |
| **Panel** | CPU, RAM, disco, batería, red y temperaturas en vivo (cada 1,5 s), con gráficos. Ícono en la barra de menú con % de batería y temperatura de CPU. |
| **Batería** | Porcentaje, salud (capacidad real vs. diseño), ciclos, temperatura, voltaje, potencia, adaptador. Límite de carga y descarga forzada (si el firmware lo permite; ver abajo). |
| **Ventiladores** | RPM en vivo, modo automático/manual con RPM fijas (experimental), ~80 sensores de temperatura agrupados. |
| **Almacenamiento** | Uso por carpeta navegable, archivos grandes, duplicados (blake3). |
| **Nube** | iCloud Drive, Google Drive, OneDrive, Dropbox, Proton y cualquier nube de `~/Library/CloudStorage`: tamaño, % en este Mac y % subido, archivos pendientes de subir y conflictos. Explorador con estado por archivo, descargar y liberar espacio local. Transferencias con barras de bajada/subida y velocidad. **Backups** a cualquier nube (espejo o versiones, con horario) y restauración. |
| **Limpieza** | Cachés, logs, Xcode, cachés de desarrollo, adjuntos de Mail, Papelera, descargas antiguas. **Borrado permanente** con vista previa, confirmación y registro. |
| **Procesos** | Tabla ordenable y buscable; terminar procesos propios. |
| **Optimizar** | Liberar RAM, vaciar DNS, reducir snapshots de Time Machine, reindexar Spotlight, limpiar cachés del sistema, activar/desactivar elementos de inicio. |

## Instalar

```bash
scripts/build-pkg.sh
```

Abre `dist/iclear-0.1.0.pkg` e instala la app en `/Applications`. Como no está firmado con una Developer ID de Apple, la primera vez se abre con **clic derecho › Abrir**. Detalles y firma en [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#instalador-pkg).

## Inicio rápido (desarrollo)

```bash
cargo build -p iclear-helper
```
```bash
cd backend/app && cargo tauri dev
```

Luego, en la app: **Ajustes › Instalar asistente** (pide tu contraseña de administrador) para habilitar ventiladores y acciones de sistema. Detalles en [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Limitación conocida: límite de carga en macOS 27

Desde macOS 27 Apple bloqueó a apps de terceros las llaves del SMC que detienen la carga. iclear lo detecta y muestra un acceso directo al **límite de carga nativo** de macOS (Ajustes del Sistema › Batería, 80–100 %). Ver [docs/SMC_KEYS.md](docs/SMC_KEYS.md).

## Estructura

```
backend/protocol   tipos compartidos
backend/core       lógica sin privilegios + cliente SMC
backend/helper     daemon root (launchd)
backend/cli        diagnóstico por terminal
backend/app        app Tauri
frontend           UI Leptos
docs/              documentación
```

## Documentación

- [Arquitectura y contrato de comandos](docs/ARCHITECTURE.md)
- [Progreso y pendientes](docs/PROGRESS.md)
- [Diseño visual](docs/DESIGN.md)
- [Inteligencia: memoria y apps sin uso](docs/INTELLIGENCE.md)
- [Nube: nubes, transferencias y backups](docs/CLOUD.md)
- [Reglas de limpieza y guardas de borrado](docs/CLEANUP_RULES.md)
- [Seguridad del helper root](docs/HELPER_SECURITY.md)
- [Llaves SMC verificadas](docs/SMC_KEYS.md)
- [Desarrollo](docs/DEVELOPMENT.md)
- [Cambios](CHANGELOG.md)
