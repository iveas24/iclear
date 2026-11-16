# Arquitectura de iclear

```
┌────────────────────────── App iclear (usuario) ──────────────────────────┐
│  frontend/ (Leptos → WASM)  ──invoke/listen──▶  backend/app (Tauri 2)     │
│                                                   │  usa                   │
│                                                   ▼                        │
│                                           backend/core (lib)               │
│                     sysinfo · ioreg · SMC (lectura) · storage · launchd    │
└───────────────────────────────────────────────────┬──────────────────────┘
                                                    │ JSON por línea
                                                    │ /var/run/iclear-helper.sock
                                                    ▼
                         backend/helper  (root, launchd com.iclear.helper)
                         SMC escritura: ventiladores, límite de carga,
                         descarga forzada · purge · DNS · tmutil · mdutil
```

| Crate | Ruta | Rol |
|---|---|---|
| `iclear-protocol` | `backend/protocol` | Tipos serde compartidos (app, helper, frontend). Sin llamadas al SO. |
| `iclear-core` | `backend/core` | Toda la lógica sin privilegios + cliente SMC (FFI a IOKit). |
| `iclear-helper` | `backend/helper` | Daemon root con comandos cerrados y validados. |
| `iclear-cli` | `backend/cli` | Diagnóstico por terminal (`battery`, `smc`, `fans`, `scan`, `memory`, `apps`…). |
| `iclear-app` | `backend/app` | App Tauri 2: comandos + evento `metrics`. |
| `iclear-frontend` | `frontend` | UI Leptos (CSR) compilada con Trunk. |

## Contrato frontend ↔ backend (comandos Tauri)

Todos los tipos están en `backend/protocol/src/lib.rs`. Los argumentos se mandan desde JS/WASM en **camelCase** (Tauri convierte `category_ids` → `categoryIds`). Los errores llegan como `string`.

| Comando | Argumentos | Devuelve |
|---|---|---|
| `get_machine_info` | — | `MachineInfo` |
| `get_metrics` | — | `Metrics` |
| `get_processes` | — | `Vec<ProcessInfo>` (ordenados por CPU) |
| `kill_process` | `pid: u32, force: bool` | `()` |
| `cleanup_categories` | — | `Vec<CleanupCategory>` (lento: calcula tamaños) |
| `preview_cleanup` | `categoryIds: Vec<String>` | `CleanupPreview` |
| `preview_paths` | `paths: Vec<String>` | `CleanupPreview` (archivos grandes / duplicados) |
| `execute_cleanup` | `token: String, paths: Vec<String>` | `DeleteReport` — **borrado permanente** |
| `disk_usage` | `path: Option<String>` (None = $HOME) | `UsageReport` |
| `large_files` | `path: Option<String>, minMb: u64, limit: usize` | `Vec<LargeFile>` |
| `find_duplicates` | `path: Option<String>, minMb: u64` | `Vec<DuplicateGroup>` |
| `reveal_in_finder` | `path: String` | `()` |
| `helper_status` | — | `HelperStatus` |
| `install_helper` | — | `HelperStatus` (pide contraseña de admin) |
| `uninstall_helper` | — | `HelperStatus` |
| `set_fan_auto` | `fan: u8` | `HelperState` |
| `set_fan_rpm` | `fan: u8, rpm: f32` | `HelperState` |
| `set_charge_limit` | `percent: Option<u8>` (None = sin límite) | `HelperState` |
| `set_force_discharge` | `enabled: bool` | `HelperState` |
| `run_optimization` | `action: OptimizeAction` (`"PurgeMemory"`, `"FlushDns"`, `"ThinLocalSnapshots"`, `"ReindexSpotlight"`, `"CleanSystemCaches"`) | `String` (mensaje) |
| `get_memory_report` | — | `MemoryReport`: desglose, apps agrupadas por proceso responsable, recomendaciones, puntaje. Ver [INTELLIGENCE.md](INTELLIGENCE.md) |
| `quit_app` | `mainPid: u32` | `()`: AppleScript `quit` por bundle id, o SIGTERM; solo procesos propios; nunca Claude |
| `get_apps_report` | — | `AppsReport`: apps instaladas con veredicto de uso, restos y huérfanos (lento: ~6 s) |
| `preview_uninstall` | `appPath: String` | `CleanupPreview` con el paquete y sus restos (guarda `check_app_removal`) |
| `preview_orphans` | `bundleIds: Vec<String>` | `CleanupPreview` con los restos huérfanos, recalculados en el backend |
| `list_launch_agents` | — | `Vec<LaunchAgent>` |
| `set_launch_agent_enabled` | `path: String, enabled: bool` | `()` |
| `open_full_disk_access_settings` | — | `()` |
| `open_battery_settings` | — | `()` (abre Ajustes › Batería, donde está el límite de carga nativo de macOS) |
| `list_cloud_providers` | — | `Vec<CloudProvider>`: iCloud Drive y cada cuenta en `~/Library/CloudStorage`. Ver [CLOUD.md](CLOUD.md) |
| `cloud_stats` | `providerId: String` | `CloudStats` (la primera llamada inicia un escaneo en segundo plano; `scanning` = cifras en aumento) |
| `rescan_cloud` | `providerId: String` | `CloudStats` (reinicia el escaneo) |
| `list_cloud_folder` | `providerId: String, path: Option<String>` | `CloudListing` con estado, % bajado y % subido por ítem |
| `cloud_download` | `path: String` | `Transfer` (descarga en segundo plano) |
| `cloud_free_up` | `path: String` | `CloudActionReport`: deja solo en la nube lo ya subido |
| `list_transfers` | — | `Vec<Transfer>` (más nuevas primero) |
| `cancel_transfer` | `id: u64` | `()` |
| `clear_transfers` | — | `()` (quita las terminadas) |
| `list_backups` | — | `Vec<BackupJob>` |
| `save_backup` | `draft: BackupDraft` | `BackupJob` (crea si `id` es `None`) |
| `delete_backup` | `id: String` | `()` (no borra las copias en la nube) |
| `run_backup` | `id: String` | `Transfer` |
| `backup_versions` | `id: String` | `Vec<BackupVersion>` |
| `restore_backup` | `id: String, versionPath: String, target: Option<String>` | `Transfer` (carpeta nueva, por defecto en `~/Downloads`) |

**Evento** `metrics` (payload `Metrics`) cada ~1,5 s desde un hilo de fondo. El mismo hilo inicia cada ~60 s los backups programados que correspondan.

## Flujo de borrado permanente
1. `cleanup_categories` → tamaños por categoría.
2. `preview_cleanup([...])` → lista exacta de ítems + `token` (válido 15 min, un solo uso).
3. La UI muestra el diálogo de confirmación (total, cantidad, “no se puede deshacer”) y permite desmarcar ítems.
4. `execute_cleanup(token, paths)` → solo borra rutas que estaban en esa vista previa **y** pasan `Guard::check`.
5. Cada borrado queda en `~/Library/Logs/iclear/deletions.log`.

Detalles de reglas en [CLEANUP_RULES.md](CLEANUP_RULES.md); seguridad del helper en [HELPER_SECURITY.md](HELPER_SECURITY.md).
